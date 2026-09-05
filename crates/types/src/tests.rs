// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Credentials ──────────────────────────────────────────────────────────

    #[test]
    fn credentials_root_is_root() {
        assert!(Credentials::ROOT.is_root());
    }

    #[test]
    fn credentials_default_is_root() {
        assert!(Credentials::default().is_root());
    }

    #[test]
    fn credentials_non_root() {
        let c = Credentials {
            uid: 1000,
            gid: 1000,
            euid: 1000,
            egid: 1000,
        };
        assert!(!c.is_root());
    }

    #[test]
    fn credentials_root_uid_zero_gid_nonzero_is_not_root() {
        // is_root requires BOTH uid AND gid to be 0.
        let c = Credentials {
            uid: 0,
            gid: 1,
            euid: 0,
            egid: 1,
        };
        assert!(!c.is_root(), "uid=0 alone does not make a process root");
    }

    // ── VmFlags → VmProt ─────────────────────────────────────────────────────

    #[test]
    fn vmflags_empty_is_none() {
        assert_eq!(VmFlags::empty().to_prot(), VmProt::None);
    }

    #[test]
    fn vmflags_read_only() {
        assert_eq!(VmFlags::READ.to_prot(), VmProt::Read);
    }

    #[test]
    fn vmflags_read_write() {
        assert_eq!(VmFlags::READ_WRITE.to_prot(), VmProt::ReadWrite);
    }

    #[test]
    fn vmflags_read_exec() {
        assert_eq!(VmFlags::READ_EXEC.to_prot(), VmProt::ReadExec);
    }

    #[test]
    fn vmflags_rwx() {
        let f = VmFlags {
            read: true,
            write: true,
            exec: true,
            ..VmFlags::empty()
        };
        assert_eq!(f.to_prot(), VmProt::ReadWriteExec);
    }

    #[test]
    fn vmflags_write_only_is_none() {
        // write without read is not a valid user mode mapping.
        let f = VmFlags {
            write: true,
            ..VmFlags::empty()
        };
        assert_eq!(f.to_prot(), VmProt::None);
    }

    // ── VMA ──────────────────────────────────────────────────────────────────

    #[test]
    fn vma_len() {
        let v = Vma {
            start: 0x1000,
            end: 0x5000,
            flags: VmFlags::READ,
            offset: 0,
            inode: 0,
            cow: false,
            mapped: true,
        };
        assert_eq!(v.len(), 0x4000);
    }

    #[test]
    fn vma_contains_inside() {
        let v = Vma {
            start: 0x1000,
            end: 0x5000,
            flags: VmFlags::empty(),
            offset: 0,
            inode: 0,
            cow: false,
            mapped: true,
        };
        assert!(v.contains(0x1000)); // inclusive start
        assert!(v.contains(0x4FFF)); // exclusive end
        assert!(v.contains(0x3000)); // middle
    }

    #[test]
    fn vma_contains_outside() {
        let v = Vma {
            start: 0x1000,
            end: 0x5000,
            flags: VmFlags::empty(),
            offset: 0,
            inode: 0,
            cow: false,
            mapped: true,
        };
        assert!(!v.contains(0x0FFF)); // just below
        assert!(!v.contains(0x5000)); // exclusive end
        assert!(!v.contains(0x5001)); // just above
        assert!(!v.contains(0)); // far below
    }

    // ── is_user_addr fallback (no provider) ──────────────────────────────────

    #[test]
    fn is_user_addr_no_provider_uses_constant() {
        // No provider registered → defaults to USER_ADDR_MAX check.
        assert!(is_user_addr(0x1000));
        assert!(is_user_addr(USER_ADDR_MAX));
        assert!(!is_user_addr(USER_ADDR_MAX + 1));
        assert!(!is_user_addr(0xFFFF_8000_0000_0000));
    }

    // ── VmProt discriminants ─────────────────────────────────────────────────

    #[test]
    fn vmprot_discriminants() {
        assert_eq!(VmProt::None as u8, 0);
        assert_eq!(VmProt::Read as u8, 1);
        assert_eq!(VmProt::Write as u8, 2);
        assert_eq!(VmProt::Exec as u8, 4);
        assert_eq!(VmProt::ReadWrite as u8, 3);
        assert_eq!(VmProt::ReadExec as u8, 5);
        assert_eq!(VmProt::ReadWriteExec as u8, 7);
    }

    // ── SocketHandle / PipeHandle ─────────────────────────────────────────────

    #[test]
    fn socket_handle_eq_and_copy() {
        let a = SocketHandle(42);
        let b = a; // Copy
        let c = SocketHandle(43);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn pipe_handle_copy_and_clone() {
        let p = PipeHandle(7);
        let q = p; // Copy
        let r = p.clone();
        assert_eq!(p, q);
        assert_eq!(p, r);
    }

    // ── IoVec ────────────────────────────────────────────────────────────────

    #[test]
    fn iovec_construction() {
        let iov = IoVec {
            base: 0xDEAD_BEEF,
            len: 4096,
        };
        assert_eq!(iov.base, 0xDEAD_BEEF);
        assert_eq!(iov.len, 4096);
    }

    // ── Constants sanity ─────────────────────────────────────────────────────

    #[test]
    fn page_size_is_4k() {
        assert_eq!(PAGE_SIZE, 4096);
    }

    #[test]
    fn higher_half_offset_matches_kernel_addr_min() {
        assert_eq!(HIGHER_HALF_OFFSET, KERNEL_ADDR_MIN);
    }

    #[test]
    fn user_addr_max_below_kernel_addr_min() {
        assert!(USER_ADDR_MAX < KERNEL_ADDR_MIN);
    }

    #[test]
    fn max_cpus_is_powers_of_two_friendly() {
        // 256 = 2^8 — convenient for bitmasks.
        assert!(MAX_CPUS.is_power_of_two());
    }

    // ── No-provider fallbacks ────────────────────────────────────────────────

    #[test]
    fn current_pid_no_provider_returns_zero() {
        // spin::Once is initialized once globally; without a kernel boot
        // having registered a provider, we get 0.
        assert_eq!(current_pid(), 0);
    }

    #[test]
    fn ticks_no_provider_returns_zero() {
        assert_eq!(ticks(), 0);
    }

    // ── Trait method signature is sound (compile-time) ───────────────────────

    fn _accepts_file_ops<T: FileOps>() {}
    #[test]
    fn file_ops_trait_compiles() {
        _accepts_file_ops::<DummyFile>();
    }

    fn _accepts_socket_ops<T: SocketOps>() {}
    #[test]
    fn socket_ops_trait_compiles() {
        _accepts_socket_ops::<DummySocket>();
    }

    /// Compile-only stub used to ensure trait bounds are satisfied.
    struct DummyFile;
    impl FileOps for DummyFile {
        fn read(&self, _: &mut [u8], _: u64) -> Result<usize, i32> {
            Ok(0)
        }
        fn write(&self, _: &[u8], _: u64) -> Result<usize, i32> {
            Ok(0)
        }
        fn seek(&self, _: u64, _: u32) -> Result<u64, i32> {
            Ok(0)
        }
        fn close(&self) -> Result<(), i32> {
            Ok(())
        }
        fn stat(&self) -> Result<FileStat, i32> {
            Err(-1)
        }
        fn mmap(&self, _: u64, _: usize, _: u32, _: u32) -> Result<VirtAddr, i32> {
            Err(-1)
        }
    }

    /// Compile-only stub used to ensure trait bounds are satisfied.
    struct DummySocket;
    impl SocketOps for DummySocket {
        fn bind(&self, _: &[u8], _: u16) -> Result<(), i32> {
            Ok(())
        }
        fn listen(&self, _: i32) -> Result<(), i32> {
            Ok(())
        }
        fn accept(&self) -> Result<(SocketHandle, [u8; 16], u16), i32> {
            Err(-1)
        }
        fn connect(&self, _: &[u8], _: u16) -> Result<(), i32> {
            Ok(())
        }
        fn send(&self, _: &[u8], _: i32) -> Result<usize, i32> {
            Ok(0)
        }
        fn recv(&self, _: &mut [u8], _: i32) -> Result<usize, i32> {
            Ok(0)
        }
        fn close(&self) -> Result<(), i32> {
            Ok(())
        }
    }
}
