//! Prints the digest of the embedded tool manifest v1 (used by tools/gen.sh to pin it).
fn main() {
    let digest = brain_protocol::tools::manifest_digest(brain_protocol::tools::manifest_v1());
    print!("{}", *digest);
}
