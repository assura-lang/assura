// ===========================================================================
// Source-level check wiring (moved from checks/format.rs)
// ===========================================================================

use assura_parser::ast::{Decl, MagicPattern};

use crate::TypeError;

/// Check codec registry declarations (G008: FMT.4).
pub fn check_codec_registry(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        let Decl::CodecRegistry(cr) = &decl.node else {
            continue;
        };

        // A52001: Check for overlapping magic byte prefixes
        let byte_patterns: Vec<(usize, &[u8])> = cr
            .codecs
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match &c.magic {
                MagicPattern::Bytes { bytes, .. } if !bytes.is_empty() => {
                    Some((i, bytes.as_slice()))
                }
                _ => None,
            })
            .collect();

        for (i, (idx_a, bytes_a)) in byte_patterns.iter().enumerate() {
            for (idx_b, bytes_b) in byte_patterns.iter().skip(i + 1) {
                let min_len = bytes_a.len().min(bytes_b.len());
                if bytes_a[..min_len] == bytes_b[..min_len] {
                    errors.push(TypeError {
                        code: "A52001".into(),
                        message: format!(
                            "overlapping magic byte patterns in codec registry `{}`: \
                             codec `{}` and codec `{}` share a common prefix",
                            cr.name, cr.codecs[*idx_a].name, cr.codecs[*idx_b].name,
                        ),
                        span: decl.span.clone(),
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }

        // A52002: Check for empty decoder names
        for codec in &cr.codecs {
            if codec.decoder.is_empty() {
                errors.push(TypeError {
                    code: "A52002".into(),
                    message: format!(
                        "codec `{}` in registry `{}` has no decoder function",
                        codec.name, cr.name,
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
    }

    errors
}
