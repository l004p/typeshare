use super::{Language, ScopedCrateTypes};
use crate::language::SupportedLanguage;
use crate::parser::{remove_dash_from_identifier, DecoratorKind, ParsedData};
use crate::rust_types::{RustTypeFormatError, SpecialRustType};
use crate::{
    rename::RenameExt,
    rust_types::{Id, RustConst, RustEnum, RustEnumVariant, RustField, RustStruct, RustTypeAlias},
};
use itertools::Itertools;
use joinery::JoinableIterator;
use lazy_format::lazy_format;
use std::{collections::BTreeSet, collections::HashMap, io::Write};

const INLINE: &str = "JvmInline";

/// All information needed for Kotlin type-code
#[derive(Default)]
pub struct Kotlin {
    /// Name of the Kotlin package
    pub package: String,
    /// Name of the Kotlin module
    pub module_name: String,
    /// The prefix to append to user-defined types
    pub prefix: String,
    /// Conversions from Rust type names to Kotlin type names.
    pub type_mappings: HashMap<String, String>,
    /// Whether or not to exclude the version header that normally appears at the top of generated code.
    /// If you aren't generating a snapshot test, this setting can just be left as a default (false)
    pub no_version_header: bool,
}

impl Language for Kotlin {
    fn type_map(&mut self) -> &HashMap<String, String> {
        &self.type_mappings
    }

    fn format_simple_type(
        &mut self,
        base: &String,
        generic_types: &[String],
    ) -> Result<String, RustTypeFormatError> {
        Ok(if let Some(mapped) = self.type_map().get(base) {
            mapped.into()
        } else if generic_types.contains(base) {
            base.into()
        } else {
            format!("{}{}", self.prefix, base)
        })
    }

    fn format_special_type(
        &mut self,
        special_ty: &SpecialRustType,
        generic_types: &[String],
    ) -> Result<String, RustTypeFormatError> {
        Ok(match special_ty {
            SpecialRustType::Vec(rtype) => {
                format!("List<{}>", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::Array(rtype, _) => {
                format!("List<{}>", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::Slice(rtype) => {
                format!("List<{}>", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::Option(rtype) => {
                format!("{}?", self.format_type(rtype, generic_types)?)
            }
            SpecialRustType::HashMap(rtype1, rtype2) => {
                format!(
                    "HashMap<{}, {}>",
                    self.format_type(rtype1, generic_types)?,
                    self.format_type(rtype2, generic_types)?
                )
            }
            SpecialRustType::Unit => "Unit".into(),
            // Char in Kotlin is 16 bits long, so we need to use String
            SpecialRustType::String | SpecialRustType::Char => "String".into(),
            // https://kotlinlang.org/docs/basic-types.html#integer-types
            SpecialRustType::I8 => "Byte".into(),
            SpecialRustType::I16 => "Short".into(),
            SpecialRustType::ISize | SpecialRustType::I32 => "Int".into(),
            SpecialRustType::I54 | SpecialRustType::I64 => "Long".into(),
            // https://kotlinlang.org/docs/basic-types.html#unsigned-integers
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

    fn begin_file(&mut self, w: &mut dyn Write, parsed_data: &ParsedData) -> std::io::Result<()> {
        if !self.package.is_empty() {
            if !self.no_version_header {
                writeln!(w, "/**")?;
                writeln!(w, " * Generated by typeshare {}", env!("CARGO_PKG_VERSION"))?;
                writeln!(w, " */")?;
                writeln!(w)?;
            }
            if parsed_data.multi_file {
                writeln!(w, "package {}.{}", self.package, parsed_data.crate_name)?;
            } else {
                writeln!(w, "package {}", self.package)?;
            }
            writeln!(w)?;
            writeln!(w, "import kotlinx.serialization.Serializable")?;
            writeln!(w, "import kotlinx.serialization.SerialName")?;
            writeln!(w)?;
        }

        Ok(())
    }

    fn write_type_alias(&mut self, w: &mut dyn Write, ty: &RustTypeAlias) -> std::io::Result<()> {
        self.write_comments(w, 0, &ty.comments)?;
        let type_name = format!("{}{}", &self.prefix, ty.id.original);

        if self.is_inline(&ty.decorators) {
            writeln!(w, "@Serializable")?;
            writeln!(w, "@JvmInline")?;
            writeln!(w, "value class {}{}(", self.prefix, ty.id.renamed)?;

            self.write_element(
                w,
                &RustField {
                    id: Id {
                        original: String::from("value"),
                        renamed: String::from("value"),
                        serde_rename: false,
                    },
                    ty: ty.r#type.clone(),
                    comments: vec![],
                    has_default: false,
                    decorators: HashMap::new(),
                },
                &[],
                false,
                match ty.is_redacted {
                    true => Visibility::Private,
                    false => Visibility::Public,
                },
            )?;

            writeln!(w)?;

            if ty.is_redacted {
                writeln!(w, ") {{")?;
                writeln!(w, "\tfun unwrap() = value")?;
                writeln!(w)?;
                writeln!(w, "\toverride fun toString(): String = \"***\"")?;
                writeln!(w, "}}")?;
            } else {
                writeln!(w, ")")?;
            }

            writeln!(w)?;
        } else {
            writeln!(
                w,
                "typealias {}{} = {}\n",
                type_name,
                (!ty.generic_types.is_empty())
                    .then(|| format!("<{}>", ty.generic_types.join(", ")))
                    .unwrap_or_default(),
                self.format_type(&ty.r#type, ty.generic_types.as_slice())
                    .map_err(std::io::Error::other)?
            )?;
        }

        Ok(())
    }

    fn write_const(&mut self, _w: &mut dyn Write, _c: &RustConst) -> std::io::Result<()> {
        todo!()
    }

    fn write_struct(&mut self, w: &mut dyn Write, rs: &RustStruct) -> std::io::Result<()> {
        self.write_comments(w, 0, &rs.comments)?;
        writeln!(w, "@Serializable")?;

        if rs.fields.is_empty() {
            // If the struct has no fields, we can define it as an static object.
            writeln!(w, "object {}{}\n", self.prefix, rs.id.renamed)?;
        } else {
            writeln!(
                w,
                "data class {}{}{} (",
                self.prefix,
                rs.id.renamed,
                (!rs.generic_types.is_empty())
                    .then(|| format!("<{}>", rs.generic_types.join(", ")))
                    .unwrap_or_default()
            )?;

            // Use @SerialName when writing the struct
            //
            // As of right now this was only written to handle fields
            // that get renamed to an ident with - in it
            let requires_serial_name = rs
                .fields
                .iter()
                .any(|f| f.id.renamed.chars().any(|c| c == '-'));

            if let Some((last, elements)) = rs.fields.split_last() {
                for f in elements.iter() {
                    self.write_element(
                        w,
                        f,
                        rs.generic_types.as_slice(),
                        requires_serial_name,
                        Visibility::Public,
                    )?;
                    writeln!(w, ",")?;
                }
                self.write_element(
                    w,
                    last,
                    rs.generic_types.as_slice(),
                    requires_serial_name,
                    Visibility::Public,
                )?;
                writeln!(w)?;
            }

            if rs.is_redacted {
                writeln!(w, ") {{")?;
                writeln!(w, "\toverride fun toString(): String = {:?}", rs.id.renamed)?;
                writeln!(w, "}}")?;
            } else {
                writeln!(w, ")")?;
            }

            writeln!(w)?;
        }
        Ok(())
    }

    fn write_enum(&mut self, w: &mut dyn Write, e: &RustEnum) -> std::io::Result<()> {
        // Generate named types for any anonymous struct variants of this enum
        self.write_types_for_anonymous_structs(w, e, &|variant_name| {
            format!("{}{}Inner", &e.shared().id.renamed, variant_name)
        })?;

        self.write_comments(w, 0, &e.shared().comments)?;
        writeln!(w, "@Serializable")?;

        let generic_parameters = (!e.shared().generic_types.is_empty())
            .then(|| format!("<{}>", e.shared().generic_types.join(", ")))
            .unwrap_or_default();

        match e {
            RustEnum::Unit(..) => {
                write!(
                    w,
                    "enum class {}{}{}(val string: String) ",
                    self.prefix,
                    &e.shared().id.renamed,
                    generic_parameters
                )?;
            }
            RustEnum::Algebraic { .. } => {
                write!(
                    w,
                    "sealed class {}{}{} ",
                    self.prefix,
                    &e.shared().id.renamed,
                    generic_parameters
                )?;
            }
        }

        writeln!(w, "{{")?;

        self.write_enum_variants(w, e)?;

        writeln!(w, "}}\n")
    }

    fn write_imports(
        &mut self,
        w: &mut dyn Write,
        imports: ScopedCrateTypes<'_>,
    ) -> std::io::Result<()> {
        for (path, ty) in imports {
            for t in ty {
                writeln!(w, "import {}.{path}.{t}", self.package)?;
            }
        }
        writeln!(w)
    }

    fn ignored_reference_types(&self) -> Vec<&str> {
        self.type_mappings.keys().map(|s| s.as_str()).collect()
    }
}

enum Visibility {
    Public,
    Private,
}

impl Kotlin {
    fn write_enum_variants(&mut self, w: &mut dyn Write, e: &RustEnum) -> std::io::Result<()> {
        match e {
            RustEnum::Unit(shared) => {
                for v in &shared.variants {
                    self.write_comments(w, 1, &v.shared().comments)?;
                    writeln!(w, "\t@SerialName({:?})", &v.shared().id.renamed)?;
                    writeln!(
                        w,
                        "\t{}({:?}),",
                        &v.shared().id.original,
                        v.shared().id.renamed
                    )?;
                }
            }
            RustEnum::Algebraic {
                content_key,
                shared,
                ..
            } => {
                for v in &shared.variants {
                    let printed_value = format!(r##""{}""##, &v.shared().id.renamed);
                    self.write_comments(w, 1, &v.shared().comments)?;
                    writeln!(w, "\t@Serializable")?;
                    writeln!(w, "\t@SerialName({})", printed_value)?;

                    let variant_name = {
                        let mut variant_name = v.shared().id.original.to_pascal_case();

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
                            write!(w, "\tobject {}", variant_name)?;
                        }
                        RustEnumVariant::Tuple { ty, .. } => {
                            write!(
                                w,
                                "\tdata class {}{}(",
                                variant_name,
                                (!e.shared().generic_types.is_empty())
                                    .then(|| format!("<{}>", e.shared().generic_types.join(", ")))
                                    .unwrap_or_default()
                            )?;
                            let variant_type = self
                                .format_type(ty, e.shared().generic_types.as_slice())
                                .map_err(std::io::Error::other)?;
                            write!(w, "val {}: {}", content_key, variant_type)?;
                            write!(w, ")")?;
                        }
                        RustEnumVariant::AnonymousStruct { shared, fields } => {
                            write!(
                                w,
                                "\tdata class {}{}(",
                                variant_name,
                                (!e.shared().generic_types.is_empty())
                                    .then(|| format!("<{}>", e.shared().generic_types.join(", ")))
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
                                false => ("<{}>", generics.iter().join_with(", ")),
                                true => (""),
                            });

                            write!(
                                w,
                                "val {}: {}{}{}Inner{}",
                                content_key,
                                self.prefix,
                                e.shared().id.original,
                                shared.id.original,
                                generics,
                            )?;
                            write!(w, ")")?;
                        }
                    }

                    writeln!(
                        w,
                        ": {}{}{}()",
                        self.prefix,
                        e.shared().id.original,
                        (!e.shared().generic_types.is_empty())
                            .then(|| format!("<{}>", e.shared().generic_types.join(", ")))
                            .unwrap_or_default()
                    )?;
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
        requires_serial_name: bool,
        visibility: Visibility,
    ) -> std::io::Result<()> {
        self.write_comments(w, 1, &f.comments)?;
        if requires_serial_name {
            writeln!(w, "\t@SerialName({:?})", &f.id.renamed)?;
        }
        let ty = match f.type_override(SupportedLanguage::Kotlin) {
            Some(type_override) => type_override.to_owned(),
            None => self
                .format_type(&f.ty, generic_types)
                .map_err(std::io::Error::other)?,
        };

        match visibility {
            Visibility::Public => write!(
                w,
                "\tval {}: {}{}",
                remove_dash_from_identifier(&f.id.renamed),
                ty,
                (f.has_default && !f.ty.is_optional())
                    .then_some("? = null")
                    .or_else(|| f.ty.is_optional().then_some(" = null"))
                    .unwrap_or_default()
            ),
            Visibility::Private => write!(
                w,
                "\tprivate val {}: {}{}",
                remove_dash_from_identifier(&f.id.renamed),
                ty,
                (f.has_default && !f.ty.is_optional())
                    .then_some("? = null")
                    .or_else(|| f.ty.is_optional().then_some(" = null"))
                    .unwrap_or_default()
            ),
        }
    }

    fn write_comment(
        &self,
        w: &mut dyn Write,
        indent: usize,
        comment: &str,
    ) -> std::io::Result<()> {
        writeln!(w, "{}/// {}", "\t".repeat(indent), comment)?;
        Ok(())
    }

    fn write_comments(
        &self,
        w: &mut dyn Write,
        indent: usize,
        comments: &[String],
    ) -> std::io::Result<()> {
        comments
            .iter()
            .try_for_each(|comment| self.write_comment(w, indent, comment))
    }

    fn is_inline(&self, decorators: &HashMap<DecoratorKind, BTreeSet<String>>) -> bool {
        match decorators.get(&DecoratorKind::Kotlin) {
            Some(kotlin_decorators) => kotlin_decorators.iter().contains(&String::from(INLINE)),
            _ => false,
        }
    }
}
