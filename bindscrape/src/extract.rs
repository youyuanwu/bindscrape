//! Extraction — clang `Entity`/`Type` → intermediate model types.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clang::{
    CallingConvention, Entity, EntityKind, Index, Type as ClangType, TypeKind,
    sonar::{self, Declaration, DefinitionValue},
};
use tracing::{debug, trace, warn};

use crate::config::PartitionConfig;
use crate::model::*;

/// Extract all declarations from a single partition into model types.
pub fn extract_partition(
    index: &Index,
    partition: &PartitionConfig,
    base_dir: &Path,
    namespace_overrides: &std::collections::HashMap<String, String>,
) -> Result<Partition> {
    let _ = namespace_overrides; // reserved for future per-API namespace overrides
    let header_path = partition.wrapper_header(base_dir);
    debug!(header = %header_path.display(), namespace = %partition.namespace, "parsing partition");

    let tu = index
        .parser(header_path.to_str().unwrap())
        .arguments(
            &partition
                .clang_args
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
        )
        .detailed_preprocessing_record(true)
        .parse()
        .map_err(|e| anyhow::anyhow!("failed to parse {}: {:?}", header_path.display(), e))?;

    let traverse_files = partition.traverse_files();
    let entities = tu.get_entity().get_children();

    let in_scope = |e: &Entity| should_emit(e, traverse_files, base_dir);

    // Extract structs
    let mut structs = Vec::new();
    for decl in sonar::find_structs(entities.clone()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        match extract_struct(&decl) {
            Ok(s) => {
                debug!(name = %s.name, fields = s.fields.len(), size = s.size, "extracted struct");
                structs.push(s);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping struct"),
        }
    }

    // Extract enums
    let mut enums = Vec::new();
    for decl in sonar::find_enums(entities.clone()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        match extract_enum(&decl) {
            Ok(en) => {
                debug!(name = %en.name, variants = en.variants.len(), "extracted enum");
                enums.push(en);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping enum"),
        }
    }

    // Extract functions
    let mut functions = Vec::new();
    for decl in sonar::find_functions(entities.clone()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        match extract_function(&decl) {
            Ok(f) => {
                debug!(name = %f.name, params = f.params.len(), "extracted function");
                functions.push(f);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping function"),
        }
    }

    // Extract typedefs
    let mut typedefs = Vec::new();
    for decl in sonar::find_typedefs(entities.clone()) {
        if !in_scope(&decl.entity) {
            continue;
        }
        match extract_typedef(&decl) {
            Ok(td) => {
                debug!(name = %td.name, "extracted typedef");
                typedefs.push(td);
            }
            Err(e) => warn!(name = %decl.name, err = %e, "skipping typedef"),
        }
    }

    // Extract #define constants
    let mut constants = Vec::new();
    for def in sonar::find_definitions(entities) {
        if !should_emit_by_location(&def.entity, traverse_files, base_dir) {
            continue;
        }
        let value = match def.value {
            DefinitionValue::Integer(negated, val) => {
                if negated {
                    ConstantValue::Signed(-(val as i64))
                } else if val <= i64::MAX as u64 {
                    ConstantValue::Signed(val as i64)
                } else {
                    ConstantValue::Unsigned(val)
                }
            }
            DefinitionValue::Real(val) => ConstantValue::Float(val),
        };
        debug!(name = %def.name, "extracted #define constant");
        constants.push(ConstantDef {
            name: def.name,
            value,
        });
    }

    tracing::info!(
        namespace = %partition.namespace,
        structs = structs.len(),
        enums = enums.len(),
        functions = functions.len(),
        typedefs = typedefs.len(),
        constants = constants.len(),
        "partition extraction complete"
    );

    Ok(Partition {
        namespace: partition.namespace.clone(),
        library: partition.library.clone(),
        structs,
        enums,
        functions,
        typedefs,
        constants,
    })
}

// ---------------------------------------------------------------------------
// Struct extraction
// ---------------------------------------------------------------------------

fn extract_struct(decl: &Declaration) -> Result<StructDef> {
    let ty = decl.entity.get_type().context("struct has no type")?;
    let size = ty.get_sizeof().unwrap_or(0);
    let align = ty.get_alignof().unwrap_or(0);

    let mut fields = Vec::new();
    for child in decl.entity.get_children() {
        if child.get_kind() != EntityKind::FieldDecl {
            continue;
        }
        let field_name = child.get_name().unwrap_or_default();
        let field_type = child.get_type().context("field has no type")?;
        let ctype = map_clang_type(&field_type)
            .with_context(|| format!("unsupported type for field '{}'", field_name))?;

        let bitfield_width = if child.is_bit_field() {
            child.get_bit_field_width()
        } else {
            None
        };
        let bitfield_offset = if child.is_bit_field() {
            child.get_offset_of_field().ok()
        } else {
            None
        };

        trace!(field = %field_name, ty = ?ctype, "  field");
        fields.push(FieldDef {
            name: field_name,
            ty: ctype,
            bitfield_width,
            bitfield_offset,
        });
    }

    Ok(StructDef {
        name: decl.name.clone(),
        size,
        align,
        fields,
    })
}

// ---------------------------------------------------------------------------
// Enum extraction
// ---------------------------------------------------------------------------

fn extract_enum(decl: &Declaration) -> Result<EnumDef> {
    let underlying = decl
        .entity
        .get_enum_underlying_type()
        .context("enum has no underlying type")?;
    let underlying_ctype = map_clang_type(&underlying).unwrap_or(CType::I32); // fallback to i32

    let mut variants = Vec::new();
    for child in decl.entity.get_children() {
        if child.get_kind() != EntityKind::EnumConstantDecl {
            continue;
        }
        let name = child.get_name().unwrap_or_default();
        let (signed, unsigned) = child.get_enum_constant_value().unwrap_or((0, 0));
        variants.push(EnumVariant {
            name,
            signed_value: signed,
            unsigned_value: unsigned,
        });
    }

    Ok(EnumDef {
        name: decl.name.clone(),
        underlying_type: underlying_ctype,
        variants,
    })
}

// ---------------------------------------------------------------------------
// Function extraction
// ---------------------------------------------------------------------------

fn extract_function(decl: &Declaration) -> Result<FunctionDef> {
    let fn_type = decl.entity.get_type().context("function has no type")?;

    let ret_type = fn_type
        .get_result_type()
        .context("function has no return type")?;
    let return_ctype = map_clang_type(&ret_type).unwrap_or(CType::Void);

    let calling_convention = fn_type
        .get_calling_convention()
        .map(map_calling_convention)
        .unwrap_or(CallConv::Cdecl);

    let args = decl.entity.get_arguments().unwrap_or_default();
    let arg_types = fn_type.get_argument_types().unwrap_or_default();

    let mut params = Vec::new();
    for (i, arg_entity) in args.iter().enumerate() {
        let name = arg_entity
            .get_name()
            .unwrap_or_else(|| format!("param{}", i));
        let ty = if i < arg_types.len() {
            map_clang_type(&arg_types[i]).unwrap_or(CType::Void)
        } else {
            CType::Void
        };
        params.push(ParamDef { name, ty });
    }

    Ok(FunctionDef {
        name: decl.name.clone(),
        return_type: return_ctype,
        params,
        calling_convention,
    })
}

// ---------------------------------------------------------------------------
// Typedef extraction
// ---------------------------------------------------------------------------

fn extract_typedef(decl: &Declaration) -> Result<TypedefDef> {
    let underlying = decl
        .entity
        .get_typedef_underlying_type()
        .context("typedef has no underlying type")?;
    let ctype = map_clang_type(&underlying).unwrap_or(CType::Void);

    Ok(TypedefDef {
        name: decl.name.clone(),
        underlying_type: ctype,
    })
}

// ---------------------------------------------------------------------------
// Type mapping: clang TypeKind → CType
// ---------------------------------------------------------------------------

fn map_clang_type(ty: &ClangType) -> Result<CType> {
    match ty.get_kind() {
        TypeKind::Void => Ok(CType::Void),
        TypeKind::Bool => Ok(CType::Bool),
        TypeKind::CharS | TypeKind::SChar => Ok(CType::I8),
        TypeKind::CharU | TypeKind::UChar => Ok(CType::U8),
        TypeKind::Short => Ok(CType::I16),
        TypeKind::UShort => Ok(CType::U16),
        TypeKind::Int => Ok(CType::I32),
        TypeKind::UInt => Ok(CType::U32),
        // C `long` → 32-bit for Windows ABI (regardless of host)
        TypeKind::Long => Ok(CType::I32),
        TypeKind::ULong => Ok(CType::U32),
        TypeKind::LongLong => Ok(CType::I64),
        TypeKind::ULongLong => Ok(CType::U64),
        TypeKind::Float => Ok(CType::F32),
        TypeKind::Double => Ok(CType::F64),

        TypeKind::Pointer => {
            let pointee = ty
                .get_pointee_type()
                .context("pointer has no pointee type")?;
            let is_const = pointee.is_const_qualified();
            let inner = map_clang_type(&pointee)?;
            Ok(CType::Ptr {
                pointee: Box::new(inner),
                is_const,
            })
        }

        TypeKind::ConstantArray => {
            let elem = ty.get_element_type().context("array has no element type")?;
            let len = ty.get_size().unwrap_or(0);
            let inner = map_clang_type(&elem)?;
            Ok(CType::Array {
                element: Box::new(inner),
                len,
            })
        }

        TypeKind::IncompleteArray => {
            // Treat as pointer
            let elem = ty
                .get_element_type()
                .context("incomplete array has no element type")?;
            let inner = map_clang_type(&elem)?;
            Ok(CType::Ptr {
                pointee: Box::new(inner),
                is_const: false,
            })
        }

        TypeKind::Elaborated => {
            let inner = ty
                .get_elaborated_type()
                .context("elaborated type has no inner type")?;
            map_clang_type(&inner)
        }

        TypeKind::Typedef => {
            let decl = ty.get_declaration();
            if let Some(decl) = decl {
                let name = decl.get_name().unwrap_or_default();
                if !name.is_empty() {
                    // Check for well-known C typedefs → map to primitives
                    match name.as_str() {
                        "int8_t" | "__int8" => return Ok(CType::I8),
                        "uint8_t" => return Ok(CType::U8),
                        "int16_t" | "__int16" => return Ok(CType::I16),
                        "uint16_t" => return Ok(CType::U16),
                        "int32_t" | "__int32" => return Ok(CType::I32),
                        "uint32_t" => return Ok(CType::U32),
                        "int64_t" | "__int64" => return Ok(CType::I64),
                        "uint64_t" => return Ok(CType::U64),
                        "size_t" | "uintptr_t" => return Ok(CType::USize),
                        "ssize_t" | "intptr_t" | "ptrdiff_t" => return Ok(CType::ISize),
                        _ => return Ok(CType::Named { name }),
                    }
                }
            }
            // Fallback: resolve underlying type
            let canonical = ty.get_canonical_type();
            map_clang_type(&canonical)
        }

        TypeKind::Record => {
            let decl = ty.get_declaration();
            if let Some(decl) = decl
                && let Some(name) = decl.get_name()
            {
                return Ok(CType::Named { name });
            }
            anyhow::bail!("anonymous record type without name")
        }

        TypeKind::Enum => {
            let decl = ty.get_declaration();
            if let Some(decl) = decl
                && let Some(name) = decl.get_name()
            {
                return Ok(CType::Named { name });
            }
            anyhow::bail!("anonymous enum type without name")
        }

        TypeKind::FunctionPrototype => {
            let ret = ty
                .get_result_type()
                .context("function prototype has no return type")?;
            let ret_ctype = map_clang_type(&ret)?;
            let arg_types = ty.get_argument_types().unwrap_or_default();
            let mut params = Vec::new();
            for at in &arg_types {
                params.push(map_clang_type(at)?);
            }
            let cc = ty
                .get_calling_convention()
                .map(map_calling_convention)
                .unwrap_or(CallConv::Cdecl);
            Ok(CType::FnPtr {
                return_type: Box::new(ret_ctype),
                params,
                calling_convention: cc,
            })
        }

        TypeKind::FunctionNoPrototype => {
            // K&R-style function — treat as void() for now
            Ok(CType::FnPtr {
                return_type: Box::new(CType::Void),
                params: vec![],
                calling_convention: CallConv::Cdecl,
            })
        }

        other => {
            anyhow::bail!("unsupported clang TypeKind: {:?}", other)
        }
    }
}

// ---------------------------------------------------------------------------
// Calling convention mapping
// ---------------------------------------------------------------------------

fn map_calling_convention(cc: CallingConvention) -> CallConv {
    match cc {
        CallingConvention::Cdecl => CallConv::Cdecl,
        CallingConvention::Stdcall => CallConv::Stdcall,
        CallingConvention::Fastcall => CallConv::Fastcall,
        // Everything else → Cdecl (platform default)
        _ => CallConv::Cdecl,
    }
}

// ---------------------------------------------------------------------------
// Source-location filtering (partition traversal)
// ---------------------------------------------------------------------------

fn should_emit(entity: &Entity, traverse_files: &[PathBuf], base_dir: &Path) -> bool {
    should_emit_by_location(entity, traverse_files, base_dir)
}

fn should_emit_by_location(entity: &Entity, traverse_files: &[PathBuf], base_dir: &Path) -> bool {
    let location = match entity.get_location() {
        Some(loc) => loc,
        None => return false,
    };
    let file_location = location.get_file_location();
    let file = match file_location.file {
        Some(f) => f,
        None => return false,
    };
    let file_path = file.get_path();

    traverse_files.iter().any(|tf| {
        let abs_tf = if tf.is_absolute() {
            tf.clone()
        } else {
            base_dir.join(tf)
        };
        // Match by canonical path or suffix
        file_path == abs_tf || file_path.ends_with(tf)
    })
}

/// Build a type registry from all partitions' extracted data.
pub fn build_type_registry(
    partitions: &[Partition],
    namespace_overrides: &std::collections::HashMap<String, String>,
) -> TypeRegistry {
    let mut registry = TypeRegistry::default();
    for partition in partitions {
        for s in &partition.structs {
            let ns = namespace_overrides
                .get(&s.name)
                .unwrap_or(&partition.namespace);
            registry.register(&s.name, ns);
        }
        for e in &partition.enums {
            let ns = namespace_overrides
                .get(&e.name)
                .unwrap_or(&partition.namespace);
            registry.register(&e.name, ns);
        }
        for td in &partition.typedefs {
            let ns = namespace_overrides
                .get(&td.name)
                .unwrap_or(&partition.namespace);
            registry.register(&td.name, ns);
        }
    }
    registry
}
