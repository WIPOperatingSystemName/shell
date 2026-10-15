//! Private inherited channels carry descriptors; passwords remain on a separate pipe.
use std::io::{self, Read, Write};
use std::os::{fd::{AsRawFd, FromRawFd, OwnedFd}, unix::net::UnixStream};

pub(super) fn send_fd(channel: &UnixStream, fd: &impl AsRawFd) -> io::Result<()> {
    let mut byte = [1_u8];
    let mut vector = libc::iovec { iov_base: byte.as_mut_ptr().cast(), iov_len: 1 };
    let mut control = [0_usize; 8];
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut vector;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = unsafe { libc::CMSG_SPACE(size_of::<libc::c_int>() as u32) } as usize;
    // Aligned storage covers the complete cmsghdr plus exactly one descriptor.
    unsafe {
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(size_of::<libc::c_int>() as u32) as usize;
        std::ptr::write(libc::CMSG_DATA(header).cast::<libc::c_int>(), fd.as_raw_fd());
        loop {
            let sent = libc::sendmsg(channel.as_raw_fd(), &message, libc::MSG_NOSIGNAL);
            if sent == 1 { return Ok(()); }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
        }
    }
}
pub(super) fn receive_fd(channel: &UnixStream) -> io::Result<OwnedFd> {
    let mut byte = [0_u8];
    let mut vector = libc::iovec { iov_base: byte.as_mut_ptr().cast(), iov_len: 1 };
    let mut control = [0_usize; 8];
    let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
    message.msg_iov = &mut vector;
    message.msg_iovlen = 1;
    message.msg_control = control.as_mut_ptr().cast();
    message.msg_controllen = size_of_val(&control);
    let mut descriptors = Vec::new();
    // Adopt every received descriptor immediately, including malformed/truncated
    // messages, so every rejection closes them rather than leaking privileged FDs.
    unsafe {
        let received = loop {
            let received = libc::recvmsg(channel.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC);
            if received >= 0 { break received; }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted { return Err(error); }
        };
        let mut header = libc::CMSG_FIRSTHDR(&message);
        while !header.is_null() {
            if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
                let length = (*header).cmsg_len.saturating_sub(libc::CMSG_LEN(0) as usize) / size_of::<libc::c_int>();
                for index in 0..length {
                    let raw = std::ptr::read(libc::CMSG_DATA(header).cast::<libc::c_int>().add(index));
                    descriptors.push(OwnedFd::from_raw_fd(raw));
                }
            }
            header = libc::CMSG_NXTHDR(&message, header);
        }
        if received != 1 || byte != [1] || message.msg_flags & libc::MSG_CTRUNC != 0 || descriptors.len() != 1 {
            return Err(io::Error::other("Invalid descriptor handoff"));
        }
    }
    Ok(descriptors.pop().unwrap())
}
pub(super) fn send(channel: &mut impl Write, value: &serde_json::Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > 65536 { return Err(io::Error::other("Session message exceeds its limit")); }
    channel.write_all(&(bytes.len() as u32).to_be_bytes())?;
    channel.write_all(&bytes)?;
    channel.flush()
}
pub(super) fn receive(channel: &mut impl Read) -> io::Result<serde_json::Value> {
    let mut length = [0; 4];
    channel.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > 65536 { return Err(io::Error::other("Session message exceeds its limit")); }
    let mut bytes = vec![0; length];
    channel.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_handoff_preserves_a_live_connection_and_sets_close_on_exec() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let (mut input, output) = UnixStream::pair().unwrap();
        send_fd(&sender, &output).unwrap();
        let fd = receive_fd(&receiver).unwrap();
        assert_ne!(unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
        drop(output);
        let mut received = UnixStream::from(fd);
        received.write_all(b"transferred").unwrap();
        let mut bytes = [0; 11];
        input.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"transferred");
    }
    #[test]
    fn missing_descriptor_and_oversized_message_are_rejected() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        sender.write_all(&[1]).unwrap();
        assert!(receive_fd(&receiver).is_err());
        assert!(receive(&mut &65537_u32.to_be_bytes()[..]).is_err());
    }
    #[test]
    fn consecutive_descriptors_do_not_consume_the_following_json_frame() {
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        let (first, second) = UnixStream::pair().unwrap();
        send_fd(&sender, &first).unwrap();
        send_fd(&sender, &second).unwrap();
        send(&mut sender, &serde_json::json!(["session", "display"])).unwrap();
        let _first = receive_fd(&receiver).unwrap();
        let _second = receive_fd(&receiver).unwrap();
        assert_eq!(receive(&mut receiver).unwrap(), serde_json::json!(["session", "display"]));
    }
}
