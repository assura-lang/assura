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
                b.iter(|| assura_types::type_check(resolved));
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
        let typed = assura_types::type_check(&resolved).expect("demo should typecheck");
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
        let typed = assura_types::type_check(&resolved).expect("demo should typecheck");
        let demo_path = format!("demos/{}.assura", demo.name);
        group.bench_with_input(
            BenchmarkId::new("verify", demo.name),
            &(typed, demo_path),
            |b, (typed, path)| {
                b.iter(|| {
                    assura_smt::Verifier::new(typed)
                        .source(std::path::Path::new(path))
                        .verify()
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
                    let (file, _) = assura_parser::parse(src);
                    let file = file.expect("parse");
                    let resolved = assura_resolve::resolve(&file).expect("resolve");
                    let typed = assura_types::type_check(&resolved).expect("typecheck");
                    let demo_path = format!("demos/{}.assura", demo.name);
                    let _results = assura_smt::Verifier::new(&typed)
                        .source(std::path::Path::new(&demo_path))
                        .verify();
                    let _project = assura_codegen::codegen(&typed);
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
                    assura_types::type_check(&resolved)
                });
            },
        );
    }
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
);
criterion_main!(benches);
