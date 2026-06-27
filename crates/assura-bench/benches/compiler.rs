use criterion::{criterion_group, criterion_main, Criterion};

fn load_demo(name: &str) -> String {
    std::fs::read_to_string(format!(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../demos/{}"),
        name
    ))
    .unwrap_or_else(|e| panic!("failed to load demo {name}: {e}"))
}

// ---------------------------------------------------------------------------
// Parse-only benchmarks
// ---------------------------------------------------------------------------

fn bench_parse(c: &mut Criterion) {
    let src_heartbleed = load_demo("heartbleed.assura");
    c.bench_function("parse_heartbleed", |b| {
        b.iter(|| assura_parser::parse(&src_heartbleed))
    });

    let src_libwebp = load_demo("libwebp-huffman.assura");
    c.bench_function("parse_libwebp", |b| {
        b.iter(|| assura_parser::parse(&src_libwebp))
    });
}

// ---------------------------------------------------------------------------
// Compile benchmarks (parse + resolve + type check, no SMT)
// ---------------------------------------------------------------------------

fn bench_compile(c: &mut Criterion) {
    let src_heartbleed = load_demo("heartbleed.assura");
    c.bench_function("compile_heartbleed", |b| {
        b.iter(|| {
            assura_pipeline::compile(
                &src_heartbleed,
                "bench.assura",
                &assura_config::CompilerConfig::default(),
            )
        })
    });

    let src_libwebp = load_demo("libwebp-huffman.assura");
    c.bench_function("compile_libwebp", |b| {
        b.iter(|| {
            assura_pipeline::compile(
                &src_libwebp,
                "bench.assura",
                &assura_config::CompilerConfig::default(),
            )
        })
    });
}

// ---------------------------------------------------------------------------
// Codegen benchmark (compile + codegen, no SMT)
// ---------------------------------------------------------------------------

fn bench_codegen(c: &mut Criterion) {
    let src = load_demo("heartbleed.assura");
    let output = assura_pipeline::compile(
        &src,
        "bench.assura",
        &assura_config::CompilerConfig::default(),
    );
    let typed = output.typed.expect("heartbleed should type-check");

    c.bench_function("codegen_heartbleed", |b| {
        b.iter(|| assura_codegen::codegen(&typed))
    });
}

// ---------------------------------------------------------------------------
// Format benchmark
// ---------------------------------------------------------------------------

fn bench_format(c: &mut Criterion) {
    let src = load_demo("heartbleed.assura");
    let (file, _) = assura_parser::parse(&src);
    let file = file.expect("heartbleed should parse");

    c.bench_function("format_heartbleed", |b| {
        b.iter(|| assura_fmt::format_source_file(&file))
    });
}

criterion_group!(benches, bench_parse, bench_compile, bench_codegen, bench_format);
criterion_main!(benches);
