fn main() {
    println!("cargo::rerun-if-changed=src/signalsmith.cpp");
    println!("cargo::rerun-if-changed=vendor");
    cc::Build::new()
        .cpp(true)
        .std("c++14")
        .include("vendor")
        .file("src/signalsmith.cpp")
        .warnings(false)
        .compile("deckforge-signalsmith");
}
