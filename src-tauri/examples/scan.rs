// `cargo run --example scan`: prints what the app would find on this PC.
fn main() {
    let s = sigf_app_lib::scan::scan();
    println!("{}", serde_json::to_string_pretty(&s).unwrap());
}
