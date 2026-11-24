// `sqlx::migrate!` embeds engine/migrations at compile time, but Cargo does
// not watch that directory: without this, a new migration file is not
// compiled in until something else in the crate changes.
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
