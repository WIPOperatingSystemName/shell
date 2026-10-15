//! Only bounded, immutable RGBA bytes cross the user/renderer boundary.
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::FileExt;

const MAX_BYTES: usize = 128 * 1024 * 1024;
const SEALS: i32 = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;

pub(crate) fn seal(pixels: &[u8]) -> Result<OwnedFd, String> {
    if pixels.is_empty() || pixels.len() > MAX_BYTES { return Err("Wallpaper exceeds the pixel limit".into()); }
    let raw = unsafe { libc::memfd_create(c"telorgon-background".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if raw < 0 { return Err(std::io::Error::last_os_error().to_string()); }
    let mut file = unsafe { File::from_raw_fd(raw) };
    file.write_all(pixels).map_err(|error| error.to_string())?;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, SEALS) } < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(file.into())
}

pub(crate) fn read(fd: OwnedFd, width: i32, height: i32) -> Result<Vec<u8>, String> {
    if !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
        return Err("Invalid wallpaper dimensions".into());
    }
    let length = (width as usize).checked_mul(height as usize).and_then(|value| value.checked_mul(4))
        .filter(|value| *value <= MAX_BYTES).ok_or("Wallpaper exceeds the pixel limit")?;
    let file = File::from(fd);
    // Check seals before size or allocation. Pipes, devices, ordinary files and
    // mutable buffers are rejected; the producer cannot change bytes mid-read.
    let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
    if seals < 0 || seals & SEALS != SEALS { return Err("Wallpaper pixels must be sealed".into()); }
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != length as u64 {
        return Err("Wallpaper pixel length does not match its dimensions".into());
    }
    let mut pixels = vec![0; length];
    // Descriptor offsets are shared with their producer. Positional reads avoid
    // trusting a seek position left or subsequently changed by that producer.
    file.read_exact_at(&mut pixels, 0).map_err(|error| error.to_string())?;
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sealed_pixels_ignore_shared_offset_and_cannot_be_changed() {
        let fd = seal(&[12, 34, 56, 255]).unwrap();
        let other = File::from(fd.try_clone().unwrap());
        assert!(other.write_at(&[0], 0).is_err());
        assert!(other.set_len(0).is_err());
        assert_eq!(read(fd, 1, 1).unwrap(), [12, 34, 56, 255]);
    }
    #[test]
    fn mutable_buffers_and_non_files_are_rejected() {
        let raw = unsafe { libc::memfd_create(c"mutable-test".as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
        assert!(raw >= 0);
        let mut file = unsafe { File::from_raw_fd(raw) };
        file.write_all(&[0; 4]).unwrap();
        assert!(read(file.into(), 1, 1).unwrap_err().contains("sealed"));
        let (socket, _) = std::os::unix::net::UnixStream::pair().unwrap();
        assert!(read(socket.into(), 1, 1).unwrap_err().contains("sealed"));
    }
    #[test]
    fn bad_sizes_are_rejected_before_allocating_or_reading() {
        for (width, height) in [(0, 1), (-1, 1), (16385, 1), (16384, 16384), (2, 1)] {
            assert!(read(seal(&[0; 4]).unwrap(), width, height).is_err());
        }
    }
}
