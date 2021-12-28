extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
  println!("cargo:rerun-if-changed=wrapper.h");

  let bindings = bindgen::Builder::default()
    .clang_arg("-target")
    .clang_arg("aarch64")
    .use_core()
    .ctypes_prefix("myctypes")
    .header("wrapper.h")
    .parse_callbacks(Box::new(bindgen::CargoCallbacks))
    .generate()
    .expect("Unable to generate bindings");

  let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
  bindings
    .write_to_file(out_path.join("bindings.rs"))
    .expect("Couldn't write bindings!");
}