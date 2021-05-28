use std::io::Write;

use anyhow::{bail, Context, Result};
use heck::{CamelCase, SnakeCase};

use crate::{
    options::Options,
    schema::{Column, SqlType, Table},
};

fn generate_prelude(dst: &mut impl Write, options: &Options) -> Result<()> {
    writeln!(dst, "// GENERATED CODE\n//")?;
    writeln!(dst, "// generated with the following options:")?;
    writeln!(
        dst,
        "//     {}\n",
        options.format().replace("\n", "\n//     ")
    )?;
    writeln!(dst, "#![allow(warnings)]")?;
    writeln!(dst, "#![allow(clippy::all)]")?;
    writeln!(dst, "\nuse clickhouse::Reflection;")?;

    Ok(())
}

fn generate_row(dst: &mut impl Write, table: &Table, options: &Options) -> Result<()> {
    writeln!(dst, "#[derive(Debug, Reflection)]")?;

    if options.serialize {
        writeln!(dst, "#[derive(serde::Serialize)]")?;
    }

    if options.deserialize {
        writeln!(dst, "#[derive(serde::Deserialize)]")?;
    }

    let mut buffer = Vec::new();

    for column in &table.columns {
        generate_field(&mut buffer, column, options)
            .with_context(|| format!("failed to generate the `{}` field", column.name))?;
    }

    let has_lifetime = buffer.windows(2).any(|w| w == b"'a");
    if has_lifetime {
        writeln!(dst, "pub struct Row<'a> {{")?;
    } else {
        writeln!(dst, "pub struct Row {{")?;
    }

    dst.write_all(&buffer)?;
    writeln!(dst, "}}")?;
    Ok(())
}

fn generate_field(dst: &mut impl Write, column: &Column, options: &Options) -> Result<()> {
    let name = column.name.to_snake_case();
    let type_ = make_type(&column.type_, &column.name, options)?;
    writeln!(dst, "    pub {}: {},", name, type_)?;
    Ok(())
}

fn make_type(raw: &SqlType, name: &str, options: &Options) -> Result<String> {
    if let Some(o) = options.overrides.iter().find(|o| o.column == name) {
        return Ok(o.type_.clone());
    }

    if let Some(t) = options.types.iter().find(|t| &t.sql == raw) {
        return Ok(t.type_.clone());
    }

    Ok(match raw {
        SqlType::UInt8 => "u8".into(),
        SqlType::UInt16 => "u16".into(),
        SqlType::UInt32 => "u32".into(),
        SqlType::UInt64 => "u64".into(),
        SqlType::Int8 => "i8".into(),
        SqlType::Int16 => "i16".into(),
        SqlType::Int32 => "i32".into(),
        SqlType::Int64 => "i64".into(),
        SqlType::String if options.owned => "String".into(),
        SqlType::String => "&'a str".into(),
        //SqlType::FixedString(size) => todo!(),
        SqlType::Float32 => "f32".into(),
        SqlType::Float64 => "f64".into(),
        //SqlType::Date => todo!(),
        //SqlType::DateTime(_) => todo!(),
        //SqlType::DateTime64(_, _) => todo!(),
        //SqlType::Ipv4 => todo!(),
        //SqlType::Ipv6 => todo!(),
        //SqlType::Uuid => todo!(),
        //SqlType::Decimal(_prec, _scale) => todo!(),
        SqlType::Enum8(_) | SqlType::Enum16(_) => name.to_camel_case(),
        SqlType::Array(inner) => format!("Vec<{}>", make_type(inner, name, options)?),
        SqlType::Tuple(inner) => inner
            .iter()
            .map(|i| make_type(i, name, options).map(|t| format!("{}, ", t)))
            .collect::<Result<_>>()?,
        //SqlType::Map(_key, _value) => todo!(),
        SqlType::Nullable(inner) => format!("Option<{}>", make_type(inner, name, options)?),
        _ => bail!(
            "there is no default impl for {}, use -T or -O to specify it",
            raw
        ),
    })
}

fn generate_enums(dst: &mut impl Write, table: &Table, options: &Options) -> Result<()> {
    fn find_enum(t: &SqlType) -> Option<(bool, &[(String, i32)])> {
        match t {
            SqlType::Enum8(v) => Some((false, &v)),
            SqlType::Enum16(v) => Some((true, &v)),
            SqlType::Array(inner) => find_enum(inner),
            SqlType::Tuple(inner) => inner.iter().flat_map(|i| find_enum(i)).next(),
            SqlType::Nullable(inner) => find_enum(inner),
            _ => None,
        }
    }

    for column in &table.columns {
        if let Some((is_extended, variants)) = find_enum(&column.type_) {
            generate_enum(
                dst,
                &column.name.to_camel_case(),
                is_extended,
                variants,
                options,
            )?;
            writeln!(dst)?;
        }
    }

    Ok(())
}

fn generate_enum(
    dst: &mut impl Write,
    name: &str,
    is_extended: bool,
    variants: &[(String, i32)],
    options: &Options,
) -> Result<()> {
    writeln!(dst, "#[derive(Debug, Reflection)]")?;

    if options.serialize {
        writeln!(dst, "#[derive(serde_repr::Serialize_repr)]")?;
    }

    if options.deserialize {
        writeln!(dst, "#[derive(serde_repr::Deserialize_repr)]")?;
    }

    if is_extended {
        writeln!(dst, "#[repr(i16)]")?;
    } else {
        writeln!(dst, "#[repr(i8)]")?;
    }

    writeln!(dst, "pub enum {} {{", name)?;

    for (name, value) in variants {
        writeln!(dst, "    {} = {},", name.to_camel_case(), value)?;
    }

    writeln!(dst, "}}")?;

    Ok(())
}

pub fn generate(table: &Table, options: &Options) -> Result<()> {
    let mut stdout = std::io::stdout();
    generate_prelude(&mut stdout, options).context("failed to generate a prelude")?;
    writeln!(stdout)?;
    generate_row(&mut stdout, table, options).context("failed to generate a row")?;
    writeln!(stdout)?;
    generate_enums(&mut stdout, table, options).context("failed to generate enums")?;
    Ok(())
}
