use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

struct DemoFile {
    name: &'static str,
    source: &'static str,
}

static DEMOS: &[DemoFile] = &[
    DemoFile {
        name: "libwebp-huffman",
        source: include_str!("../../../demos/libwebp-huffman.assura"),
    },
    DemoFile {
        name: "zlib-inflate",
        source: include_str!("../../../demos/zlib-inflate.assura"),
    },
    DemoFile {
        name: "mbedtls-x509",
        source: include_str!("../../../demos/mbedtls-x509.assura"),
    },
];

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for demo in DEMOS {
        group.bench_with_input(
            BenchmarkId::new("parse", demo.name),
            &demo.source,
            |b, src| {
                b.iter(|| assura_parser::parse(src));
            },
        );
    }
    group.finish();
}

fn bench_resolve(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolve");
    for demo in DEMOS {
        let (file, _) = assura_parser::parse(demo.source);
        let file = file.expect("demo should parse");
        group.bench_with_input(BenchmarkId::new("resolve", demo.name), &file, |b, file| {
            b.iter(|| assura_resolve::resolve(file));
        });
    }
    group.finish();
}

fn bench_type_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("type_check");
    for demo in DEMOS {
        let (file, _) = assura_parser::parse(demo.source);
        let file = file.expect("demo should parse");
        let resolved = assura_resolve::resolve(&file).expect("demo should resolve");
        group.bench_with_input(
            BenchmarkId::new("typecheck", demo.name),
            &resolved,
            |b, resolved| {
                b.iter(|| assura_types::type_check(resolved.clone()));
            },
        );
    }
    group.finish();
}

fn bench_codegen(c: &mut Criterion) {
    let mut group = c.benchmark_group("codegen");
    for demo in DEMOS {
        let (file, _) = assura_parser::parse(demo.source);
        let file = file.expect("demo should parse");
        let resolved = assura_resolve::resolve(&file).expect("demo should resolve");
        let typed = assura_types::type_check(resolved).expect("demo should typecheck");
        group.bench_with_input(
            BenchmarkId::new("codegen", demo.name),
            &typed,
            |b, typed| {
                b.iter(|| assura_codegen::codegen(typed));
            },
        );
    }
    group.finish();
}

fn bench_smt_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("smt_verify");
    group.sample_size(20); // SMT queries are slower
    for demo in DEMOS {
        let (file, _) = assura_parser::parse(demo.source);
        let file = file.expect("demo should parse");
        let resolved = assura_resolve::resolve(&file).expect("demo should resolve");
        let typed = assura_types::type_check(resolved).expect("demo should typecheck");
        let demo_path = format!("demos/{}.assura", demo.name);
        group.bench_with_input(
            BenchmarkId::new("verify", demo.name),
            &(typed, demo_path),
            |b, (typed, path)| {
                b.iter(|| {
                    let config = assura_config::CompilerConfig::default();
                    assura_pipeline::verify_typed(typed, path, &config)
                });
            },
        );
    }
    group.finish();
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");
    group.sample_size(20);
    for demo in DEMOS {
        group.bench_with_input(
            BenchmarkId::new("pipeline", demo.name),
            &demo.source,
            |b, src| {
                b.iter(|| {
                    let demo_path = format!("demos/{}.assura", demo.name);
                    let config = assura_config::CompilerConfig::default();
                    let out = assura_pipeline::compile_full(src, &demo_path, &config);
                    let _ = &out.verification;
                    let _ = &out.generated;
                });
            },
        );
    }
    group.finish();
}

// Synthetic large contract for scaling tests
fn generate_large_contract(n_clauses: usize) -> String {
    let mut s = String::from("contract LargeContract {\n");
    s.push_str("  input { x: Int, y: Int }\n");
    s.push_str("  output { result: Int }\n");
    for i in 0..n_clauses {
        s.push_str(&format!("  requires {{ x + {} > 0 }}\n", i));
    }
    s.push_str("  ensures { result > 0 }\n");
    s.push_str("}\n");
    s
}

fn bench_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("scaling");
    for n in [10, 50, 100] {
        let source = generate_large_contract(n);
        group.bench_with_input(BenchmarkId::new("parse_clauses", n), &source, |b, src| {
            b.iter(|| assura_parser::parse(src));
        });
        let (file, _) = assura_parser::parse(&source);
        let file = file.expect("should parse");
        group.bench_with_input(
            BenchmarkId::new("typecheck_clauses", n),
            &file,
            |b, file| {
                b.iter(|| {
                    let resolved = assura_resolve::resolve(file).expect("resolve");
                    assura_types::type_check(resolved)
                });
            },
        );
    }
    group.finish();
}

// Generate multiple small contracts (tests contract-count scaling)
fn generate_multi_contract(n_contracts: usize) -> String {
    let mut s = String::new();
    for i in 0..n_contracts {
        s.push_str(&format!(
            "contract C{i} {{\n  input {{ x: Int, y: Int }}\n  output {{ result: Int }}\n  requires {{ x >= 0 }}\n  requires {{ y > 0 }}\n  ensures {{ x + y >= 0 }}\n}}\n\n"
        ));
    }
    s
}

// Large-scale scaling benchmarks (500, 1000, 5000 clauses)
fn bench_large_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_scaling");
    group.sample_size(10);
    for n in [500, 1000, 5000] {
        let source = generate_large_contract(n);
        group.bench_with_input(BenchmarkId::new("parse_clauses", n), &source, |b, src| {
            b.iter(|| assura_parser::parse(src));
        });
    }
    // Type-check scaling (limited to 1000 to keep benchmark runtime reasonable)
    for n in [500, 1000] {
        let source = generate_large_contract(n);
        let (file, _) = assura_parser::parse(&source);
        let file = file.expect("should parse");
        group.bench_with_input(
            BenchmarkId::new("typecheck_clauses", n),
            &file,
            |b, file| {
                b.iter(|| {
                    let resolved = assura_resolve::resolve(file).expect("resolve");
                    assura_types::type_check(resolved)
                });
            },
        );
    }
    group.finish();
}

// Multi-contract scaling (many small contracts in one file)
fn bench_multi_contract(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_contract");
    group.sample_size(10);
    for n in [50, 100, 500] {
        let source = generate_multi_contract(n);
        group.bench_with_input(BenchmarkId::new("parse_contracts", n), &source, |b, src| {
            b.iter(|| assura_parser::parse(src));
        });
        let (file, _) = assura_parser::parse(&source);
        let file = file.expect("should parse");
        group.bench_with_input(
            BenchmarkId::new("typecheck_contracts", n),
            &file,
            |b, file| {
                b.iter(|| {
                    let resolved = assura_resolve::resolve(file).expect("resolve");
                    assura_types::type_check(resolved)
                });
            },
        );
    }
    group.finish();
}

// Benchmark the large fixture file (bench_large.assura)
fn bench_large_fixture(c: &mut Criterion) {
    let source = include_str!("../../../tests/fixtures/bench_large.assura");
    let mut group = c.benchmark_group("large_fixture");
    group.sample_size(20);
    group.bench_function("parse", |b| {
        b.iter(|| assura_parser::parse(source));
    });
    let (file, _) = assura_parser::parse(source);
    let file = file.expect("should parse");
    let resolved = assura_resolve::resolve(&file).expect("should resolve");
    group.bench_function("typecheck", |b| {
        b.iter(|| assura_types::type_check(resolved.clone()));
    });
    let typed = assura_types::type_check(resolved).expect("should typecheck");
    group.bench_function("codegen", |b| {
        b.iter(|| assura_codegen::codegen(&typed));
    });
    group.finish();
}

// Benchmark multi-file project (bench_project/)
fn bench_multi_file_project(c: &mut Criterion) {
    static PROJECT_FILES: &[(&str, &str)] = &[
        (
            "math_ops",
            include_str!("../../../tests/fixtures/bench_project/math_ops.assura"),
        ),
        (
            "validation",
            include_str!("../../../tests/fixtures/bench_project/validation.assura"),
        ),
        (
            "network",
            include_str!("../../../tests/fixtures/bench_project/network.assura"),
        ),
        (
            "storage",
            include_str!("../../../tests/fixtures/bench_project/storage.assura"),
        ),
    ];

    let mut group = c.benchmark_group("multi_file_project");
    group.sample_size(20);

    // Parse all files
    group.bench_function("parse_all", |b| {
        b.iter(|| {
            for (_, src) in PROJECT_FILES {
                let _ = assura_parser::parse(src);
            }
        });
    });

    // Type-check all files
    let parsed: Vec<_> = PROJECT_FILES
        .iter()
        .map(|(name, src)| {
            let (file, _) = assura_parser::parse(src);
            (*name, file.expect("should parse"))
        })
        .collect();
    group.bench_function("typecheck_all", |b| {
        b.iter(|| {
            for (_, file) in &parsed {
                let resolved = assura_resolve::resolve(file).expect("resolve");
                let _ = assura_types::type_check(resolved);
            }
        });
    });

    // Full pipeline (compile) for all files
    group.bench_function("compile_all", |b| {
        b.iter(|| {
            for (name, src) in PROJECT_FILES {
                let config = assura_config::CompilerConfig::default();
                let _ = assura_pipeline::compile(src, *name, &config);
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_resolve,
    bench_type_check,
    bench_codegen,
    bench_smt_verify,
    bench_full_pipeline,
    bench_scaling,
    bench_large_scaling,
    bench_multi_contract,
    bench_large_fixture,
    bench_multi_file_project,
);
criterion_main!(benches);
