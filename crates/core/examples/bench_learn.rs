//! Times a cold and a warm `learn` on a generated corpus.
//!
//! The corpus is created in a temp directory so the measurement does not
//! depend on the size of this repository. Run:
//!
//! ```text
//! cargo run --release -p repowalk-core --example bench_learn
//! ```
//!
//! Prints `cold_ms`, `warm_ms`, and how many files each pass re-analyzed.
use repowalk_core::learn;
use std::fs;
use std::path::Path;
use std::time::Instant;

fn main() {
    let root = std::env::temp_dir().join(format!("repowalk-bench-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("corpus dir");
    write_corpus(&root);

    let cold_start = Instant::now();
    learn::learn(&root, &|_| {}).expect("cold learn");
    let cold_ms = cold_start.elapsed().as_millis();
    let cold_re = learn::reanalyzed(&root);

    let warm_start = Instant::now();
    learn::learn(&root, &|_| {}).expect("warm learn");
    let warm_ms = warm_start.elapsed().as_millis();
    let warm_re = learn::reanalyzed(&root);

    println!("cold_ms {cold_ms} reanalyzed {cold_re}");
    println!("warm_ms {warm_ms} reanalyzed {warm_re}");
    let _ = fs::remove_dir_all(&root);
}

/// 80 Rust files and 40 Python files, each with several small functions.
///
/// Kept deterministic: the same bytes every run, no clock, no network.
fn write_corpus(root: &Path) {
    let rust_dir = root.join("src");
    let py_dir = root.join("py");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&py_dir).unwrap();
    for i in 0..80 {
        fs::write(rust_dir.join(format!("m{i}.rs")), rust_file(i)).unwrap();
    }
    for i in 0..40 {
        fs::write(py_dir.join(format!("m{i}.py")), python_file(i)).unwrap();
    }
}

fn rust_file(i: usize) -> String {
    let mut out = String::from("//! bench module\n");
    for k in 0..12 {
        out.push_str(&format!(
            r#"
pub fn f{i}_{k}(n: i32) -> i32 {{
    let mut total = n;
    if total > {k} {{
        for step in 0..total {{
            if step % 2 == 0 && step < 10 || step > 100 {{
                total += step;
            }} else if step == {k} {{
                total -= 1;
            }}
        }}
    }}
    total
}}
"#
        ));
    }
    out
}

fn python_file(i: usize) -> String {
    let mut out = String::from("\"\"\"bench module\"\"\"\n");
    for k in 0..8 {
        out.push_str(&format!(
            r#"
def f{i}_{k}(n):
    total = n
    if total > {k} and total < 1000:
        for step in range(total):
            if step % 2 == 0 or step == {k}:
                total += step
            elif step < 3:
                total -= 1
    try:
        return total
    except Exception:
        return 0
"#
        ));
    }
    out
}
