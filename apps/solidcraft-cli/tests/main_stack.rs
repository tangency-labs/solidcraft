//! The Windows CLI asks for an 8 MiB main stack (#15438): it evaluates features on its main
//! thread, and fillet certification overflows the 1 MiB Windows gives a main thread by default.

/// The stack the kernel asks for (`KERNEL_STACK` in crates/kernel/src/par.rs).
#[cfg(all(windows, target_env = "msvc"))]
const KERNEL_STACK: u64 = 8 << 20;

/// `SizeOfStackReserve` from the optional header of the exe at `path`: 8 bytes at offset 72 in a
/// PE32+ image (64-bit), 4 bytes there in a PE32 image (32-bit).
#[cfg(all(windows, target_env = "msvc"))]
fn stack_reserve(path: &str) -> u64 {
    let bytes = std::fs::read(path).expect("read the built exe");
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "a PE file");
    let optional = pe + 24;
    match u16::from_le_bytes([bytes[optional], bytes[optional + 1]]) {
        0x20b => u64::from_le_bytes(bytes[optional + 72..optional + 80].try_into().expect("8 bytes")),
        0x10b => u64::from(u32_at(optional + 72)),
        magic => panic!("{path}: optional header magic {magic:#x} is neither PE32 nor PE32+"),
    }
}

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_cli_reserves_the_kernel_stack_for_its_main_thread() {
    for exe in [env!("CARGO_BIN_EXE_solidcraft-cli"), env!("CARGO_BIN_EXE_solvecraft-cli")] {
        assert!(stack_reserve(exe) >= KERNEL_STACK, "{exe}: main stack {} under 8 MiB", stack_reserve(exe));
    }
}
