use super::{CrateTypes, Language};
use crate::language::SupportedLanguage;
use crate::parser::{remove_dash_from_identifier, ParsedData};
use crate::rust_types::{
    RustConst, RustEnum, RustEnumVariant, RustField, RustStruct, RustType, RustTypeAlias,
    RustTypeFormatError, SpecialRustType,
};
use itertools::Itertools;
use joinery::JoinableIterator;
use lazy_format::lazy_format;
use std::ops::Deref;
use std::{collections::HashMap, io::Write};

/// All information needed for Scala type-code
#[derive(Default)]
pub struct Scala {
    /// Name of the Scala package
    pub package: String,
    /// Name of the Scala module
    pub module_name: String,
    /// Conversions from Rust type names to Scala type names.
    pub type_mappings: HashMap<String, String>,
    /// Whether or not to exclude the version header that normally appears at the top of generated code.
    /// If you aren't generating a snapshot test, this setting can just be left as a default (false)
    pub no_version_header: bool,
}

impl Language for Scala {
    fn generate_types(
        &mut self,
        writable: &mut dyn Write,
        _imports: &CrateTypes,
        data: ParsedData,
    ) -> std::io::Result<()> {
        self.begin_file(writable, &data)?;

        // Package object to hold type aliases: aliases must be in class or object in Scala 2)
        let unsigned_used = self.unsigned_integer_used(&data);
        if unsigned_used || !data.aliases.is_empty() {
            self.begin_package_object(writable)?;
            if unsigned_used {
                self.write_unsigned_aliases(writable)?;
            }
            for a in data.aliases.iter() {
                self.write_type_alias(writable, a)?;
            }
            self.end_package_object(writable)?;
        }

        if !data.structs.is_empty() || !data.enums.is_empty() {
            self.begin_package(writable)?;
            for s in data.structs.iter() {
                self.write_struct(writable, s)?;
            }
            for e in data.enums.iter() {
                self.write_enum(writable, e)?;
            }
            self.end_package(writable)?;
        }

        self.end_file(writable)?;

        Ok(())
    }

    fn type_map(&mut self) -> &HashMap<String, String> {
        &self.type_mappings
    }

    fn format_generic_parameters(&mut self, parameters: Vec<String>) -> String {
        format!("[{}]", parameters.into_iter().join(", "))
    }

    fn format_special_type(
        &mut self,
        special_ty: &SpecialRustType,
        generic_types: &[String],
    ) -> Result<String, RustTypeFormatError> {
        Ok(match special_ty {
            SpecialRustType::Vec(rtype) => {
                format!("Vector[{}]", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::Array(rtype, _) => {
                format!("Vector[{}]", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::Slice(rtype) => {
                format!("Vector[{}]", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::Option(rtype) => {
                format!("Option[{}]", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::HashMap(rtype1, rtype2) => {
                format!(
                    "Map[{}, {}]",
                    self.format_type(rtype1, generic_types)?,
                    self.format_type(rtype2, generic_types)?
                )
            }
            SpecialRustType::Unit => "Unit".into(),
            // Char in Scala is 16 bits long, so we need to use String
            // https://docs.scala-lang.org/scala3/book/first-look-at-types.html#scalas-value-types
            SpecialRustType::String | SpecialRustType::Char => "String".into(),
            SpecialRustType::I8 => "Byte".into(),
            SpecialRustType::I16 => "Short".into(),
            SpecialRustType::ISize | SpecialRustType::I32 => "Int".into(),
            SpecialRustType::I54 | SpecialRustType::I64 => "Long".into(),
            // Scala does not support unsigned integers, so upcast it to the closest one
            SpecialRustType::U8 => "UByte".into(),
            SpecialRustType::U16 => "UShort".into(),
            SpecialRustType::USize | SpecialRustType::U32 => "UInt".into(),
            SpecialRustType::U53 | SpecialRustType::U64 => "ULong".into(),
            SpecialRustType::Bool => "Boolean".into(),
            SpecialRustType::F32 => "Float".into(),
            SpecialRustType::F64 => "Double".into(),
            // TODO: https://github.com/1Password/typeshare/issues/237
            SpecialRustType::DateTime => {
                return Err(RustTypeFormatError::UnsupportedSpecialType(
                    special_ty.to_string(),
                ))
            }
        })
    }

    fn begin_file(&mut self, w: &mut dyn Write, _parsed_data: &ParsedData) -> std::io::Result<()> {
        if !self.no_version_header {
            writeln!(w, "/**")?;
            writeln!(w, " * Generated by typeshare {}", env!("CARGO_PKG_VERSION"))?;
            writeln!(w, " */")?;
        }
        if self.package.is_empty() {
            panic!("package name must be provided")
        }
        match self.package.rsplit_once('.') {
            None => {}
            Some((parent, _last)) => {
                writeln!(w, "package {}", parent)?;
                writeln!(w)?;
            }
        };
        Ok(())
    }

    fn write_type_alias(&mut self, w: &mut dyn Write, ty: &RustTypeAlias) -> std::io::Result<()> {
        self.write_comments(w, 0, &ty.comments)?;

        writeln!(
            w,
            "type {}{} = {}\n",
            ty.id.original,
            (!ty.generic_types.is_empty())
                .then(|| format!("[{}]", ty.generic_types.join(", ")))
                .unwrap_or_default(),
            self.format_type(&ty.r#type, ty.generic_types.as_slice())
                .map_err(std::io::Error::other)?
        )?;

        Ok(())
    }

    fn write_const(&mut self, _w: &mut dyn Write, _c: &RustConst) -> std::io::Result<()> {
        todo!()
    }

    fn write_struct(&mut self, w: &mut dyn Write, rs: &RustStruct) -> std::io::Result<()> {
        self.write_comments(w, 0, &rs.comments)?;

        if !rs.fields.is_empty() {
            writeln!(
                w,
                "case class {}{} (",
                rs.id.renamed,
                (!rs.generic_types.is_empty())
                    .then(|| format!("[{}]", rs.generic_types.join(", ")))
                    .unwrap_or_default()
            )?;

            if let Some((last, elements)) = rs.fields.split_last() {
                for f in elements.iter() {
                    self.write_element(w, f, rs.generic_types.as_slice())?;
                    writeln!(w, ",")?;
                }
                self.write_element(w, last, rs.generic_types.as_slice())?;
                writeln!(w)?;
            }
            writeln!(w, ")\n")?;
        } else {
            writeln!(w, "class {} extends Serializable\n", rs.id.renamed)?;
        }
        Ok(())
    }

    fn write_enum(&mut self, w: &mut dyn Write, e: &RustEnum) -> std::io::Result<()> {
        // Generate named types for any anonymous struct variants of this enum
        self.write_types_for_anonymous_structs(w, e, &|variant_name| {
            format!("{}{}Inner", &e.shared().id.renamed, variant_name)
        })?;

        self.write_comments(w, 0, &e.shared().comments)?;

        let generic_parameters = (!e.shared().generic_types.is_empty())
            .then(|| format!("[{}]", e.shared().generic_types.join(", ")))
            .unwrap_or_default();

        match e {
            RustEnum::Unit(shared) => {
                writeln!(
                    w,
                    "sealed trait {}{} {{",
                    shared.id.renamed, generic_parameters
                )?;
            }
            RustEnum::Algebraic { shared, .. } => {
                writeln!(
                    w,
                    "sealed trait {}{} {{",
                    shared.id.renamed, generic_parameters
                )?;
            }
        }
        writeln!(w, "\tdef serialName: String")?;
        writeln!(w, "}}")?;

        writeln!(w, "object {} {{", &e.shared().id.renamed)?;
        self.write_enum_variants(w, e)?;
        writeln!(w, "}}\n")
    }

    fn write_imports(
        &mut self,
        _writer: &mut dyn Write,
        _imports: super::ScopedCrateTypes<'_>,
    ) -> std::io::Result<()> {
        unimplemented!()
    }
}

impl Scala {
    fn write_enum_variants(&mut self, w: &mut dyn Write, e: &RustEnum) -> std::io::Result<()> {
        match e {
            RustEnum::Unit(shared) => {
                for v in shared.variants.iter() {
                    self.write_comments(w, 1, &v.shared().comments)?;
                    writeln!(
                        w,
                        "\tcase object {} extends {} {{",
                        &v.shared().id.original,
                        &e.shared().id.renamed
                    )?;
                    writeln!(
                        w,
                        "\t\tval serialName: String = {:?}",
                        v.shared().id.renamed
                    )?;
                    writeln!(w, "\t}}")?;
                }
            }
            RustEnum::Algebraic {
                content_key,
                shared,
                ..
            } => {
                for v in shared.variants.iter() {
                    let printed_value = format!(r##"{:?}"##, &v.shared().id.renamed);
                    self.write_comments(w, 1, &v.shared().comments)?;

                    let variant_name = {
                        let mut variant_name = v.shared().id.original.to_string();

                        if variant_name
                            .chars()
                            .next()
                            .map(|c| c.is_ascii_digit())
                            .unwrap_or(false)
                        {
                            // If the name starts with a digit just add an underscore
                            // to the front and make it valid
                            variant_name = format!("_{}", variant_name);
                        }

                        variant_name
                    };

                    match v {
                        RustEnumVariant::Unit(_) => {
                            write!(w, "\tcase object {}", variant_name)?;
                        }
                        RustEnumVariant::Tuple { ty, .. } => {
                            write!(
                                w,
                                "\tcase class {}{}(",
                                variant_name,
                                (!e.shared().generic_types.is_empty())
                                    .then(|| format!("[{}]", e.shared().generic_types.join(", ")))
                                    .unwrap_or_default()
                            )?;
                            let variant_type = self
                                .format_type(ty, e.shared().generic_types.as_slice())
                                .map_err(std::io::Error::other)?;
                            write!(w, "{}: {}", content_key, variant_type)?;
                            write!(w, ")")?;
                        }
                        RustEnumVariant::AnonymousStruct { shared, fields } => {
                            write!(
                                w,
                                "\tcase class {}{}(",
                                variant_name,
                                (!e.shared().generic_types.is_empty())
                                    .then(|| format!("[{}]", e.shared().generic_types.join(", ")))
                                    .unwrap_or_default()
                            )?;

                            // Builds the list of generic types (e.g [T, U, V]), by digging
                            // through the fields recursively and comparing against the
                            // enclosing enum's list of generic parameters.
                            let generics = fields
                                .iter()
                                .flat_map(|field| {
                                    e.shared()
                                        .generic_types
                                        .iter()
                                        .filter(|g| field.ty.contains_type(g))
                                })
                                .unique()
                                .collect_vec();

                            // Sadly the parenthesis are required because of macro limitations
                            let generics = lazy_format!(match (generics.is_empty()) {
                                false => ("[{}]", generics.iter().join_with(", ")),
                                true => (""),
                            });

                            write!(
                                w,
                                "{}: {}{}Inner{}",
                                content_key,
                                e.shared().id.original,
                                shared.id.original,
                                generics,
                            )?;
                            write!(w, ")")?;
                        }
                    }

                    writeln!(
                        w,
                        " extends {}{} {{",
                        e.shared().id.original,
                        (!e.shared().generic_types.is_empty())
                            .then(|| format!("[{}]", e.shared().generic_types.join(", ")))
                            .unwrap_or_default()
                    )?;
                    writeln!(w, "\t\tval serialName: String = {}", printed_value)?;
                    writeln!(w, "\t}}")?;
                }
            }
        }

        Ok(())
    }

    fn write_element(
        &mut self,
        w: &mut dyn Write,
        f: &RustField,
        generic_types: &[String],
    ) -> std::io::Result<()> {
        self.write_comments(w, 1, &f.comments)?;

        let ty = match f.type_override(SupportedLanguage::Scala) {
            Some(type_override) => type_override.to_owned(),
            None => self
                .format_type(&f.ty, generic_types)
                .map_err(std::io::Error::other)?,
        };

        write!(
            w,
            "\t{}: {}{}",
            remove_dash_from_identifier(&f.id.renamed),
            ty,
            (f.has_default && !f.ty.is_optional())
                .then_some(" = _")
                .or_else(|| f.ty.is_optional().then_some(" = None"))
                .unwrap_or_default()
        )
    }

    fn write_comment(
        &mut self,
        w: &mut dyn Write,
        indent: usize,
        comment: &str,
    ) -> std::io::Result<()> {
        writeln!(w, "{}// {}", "\t".repeat(indent), comment)?;
        Ok(())
    }

    fn write_comments(
        &mut self,
        w: &mut dyn Write,
        indent: usize,
        comments: &[String],
    ) -> std::io::Result<()> {
        comments
            .iter()
            .try_for_each(|comment| self.write_comment(w, indent, comment))
    }

    fn begin_package_object(&mut self, w: &mut dyn Write) -> std::io::Result<()> {
        match self.package.rsplit_once('.') {
            None => {}
            Some((_parent, last)) => {
                writeln!(w, "package object {} {{", last)?;
                writeln!(w)?;
            }
        };
        Ok(())
    }

    fn begin_package(&mut self, w: &mut dyn Write) -> std::io::Result<()> {
        match self.package.rsplit_once('.') {
            None => {}
            Some((_parent, last)) => {
                writeln!(w, "package {} {{", last)?;
                writeln!(w)?;
            }
        };
        Ok(())
    }

    fn write_unsigned_aliases(&mut self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "type UByte = Byte")?;
        writeln!(w, "type UShort = Short")?;
        writeln!(w, "type UInt = Int")?;
        writeln!(w, "type ULong = Int")?;
        writeln!(w)?;
        Ok(())
    }

    fn end_package_object(&mut self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "}}")?;
        Ok(())
    }

    fn end_package(&mut self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "}}")?;
        Ok(())
    }

    fn unsigned_integer_used(&mut self, data: &ParsedData) -> bool {
        let types_in_aliases = data.aliases.iter().map(|f| f.r#type.clone()).collect_vec();
        let types_in_structs = data
            .structs
            .iter()
            .flat_map(|f| f.fields.clone())
            .map(|f| f.ty)
            .collect_vec();
        let types_in_enum = data
            .enums
            .iter()
            .flat_map(|e| {
                e.shared().variants.iter().flat_map(|v| match v {
                    RustEnumVariant::Unit(_) => vec![],
                    RustEnumVariant::Tuple { ty, .. } => vec![ty.clone()],
                    RustEnumVariant::AnonymousStruct { fields, .. } => {
                        fields.iter().map(|f| f.ty.clone()).collect_vec()
                    }
                })
            })
            .collect_vec();
        itertools::concat(vec![types_in_aliases, types_in_structs, types_in_enum])
            .iter()
            .flat_map(|ty| match ty {
                RustType::Generic { id: _, parameters } => parameters.clone(),
                RustType::Special(SpecialRustType::Option(ty) | SpecialRustType::Vec(ty)) => {
                    vec![ty.deref().clone()]
                }
                RustType::Special(SpecialRustType::HashMap(kty, vty)) => {
                    vec![kty.deref().clone(), vty.deref().clone()]
                }
                RustType::Special(_) => vec![ty.clone()],
                RustType::Simple { .. } => vec![],
            })
            .any(|ty| {
                matches!(
                    ty,
                    RustType::Special(
                        SpecialRustType::U8
                            | SpecialRustType::U16
                            | SpecialRustType::U32
                            | SpecialRustType::U53
                            | SpecialRustType::U64
                            | SpecialRustType::USize,
                    )
                )
            })
    }
}
