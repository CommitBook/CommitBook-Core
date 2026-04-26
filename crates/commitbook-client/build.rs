fn main() {
    uniffi::generate_scaffolding("src/commitbook.udl")
        .expect("Failed to generate UniFFI scaffolding");
}
