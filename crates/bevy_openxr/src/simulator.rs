/// NOTE: `-rdynamic` linker arg required on the final binary for this to work

#[no_mangle]
pub extern "C" fn sim_openxr_test() {
    println!("hi");
}

pub fn init() {
    let funcs: &[*const extern "C" fn()] = &[sim_openxr_test as _];
    std::mem::forget(std::hint::black_box(funcs));
}
