fn main() {
    println!("cargo:rustc-link-lib=crypto");
    println!("cargo:rustc-link-lib=ssl");
}
