extern crate vergen;
use vergen::{generate_cargo_keys, ConstantsFlags};

fn main() {
    let mut flags = ConstantsFlags::all();
    flags.toggle(ConstantsFlags::REBUILD_ON_HEAD_CHANGE);
    generate_cargo_keys(ConstantsFlags::all()).expect("Unable to generate the cargo keys!");
}
