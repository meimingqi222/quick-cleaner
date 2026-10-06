//! Bounded command process trees and pipe reads; no background mutation survives timeout.

#[cfg(windows)]
mod native {
    use std::io::{self, Read};
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::jobapi2::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
    };
    use winapi::um::namedpipeapi::PeekNamedPipe;
    use winapi::um::processthreadsapi::{OpenThread, ResumeThread};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use winapi::um::winbase::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
    use winapi::um::winnt::{
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, THREAD_SUSPEND_RESUME,
    };

    pub fn spawn(command: &mut Command) -> Option<(Child, OwnedHandle)> {
        let job = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if job.is_null() {
            return None;
        }
        let job = unsafe { OwnedHandle::from_raw_handle(job.cast()) };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle().cast(),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return None;
        }
        // Suspended launch closes the spawn/assign race: no tool code runs outside the job.
        command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
        let mut child = command.spawn().ok()?;
        if unsafe {
            AssignProcessToJobObject(job.as_raw_handle().cast(), child.as_raw_handle().cast())
        } == 0
            || !resume_primary_thread(child.id())
        {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        Some((child, job))
    }

    fn resume_primary_thread(pid: u32) -> bool {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return false;
        }
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot.cast()) };
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut found = unsafe { Thread32First(snapshot.as_raw_handle().cast(), &mut entry) };
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if thread.is_null() {
                    return false;
                }
                let thread = unsafe { OwnedHandle::from_raw_handle(thread.cast()) };
                return unsafe { ResumeThread(thread.as_raw_handle().cast()) } == 1;
            }
            found = unsafe { Thread32Next(snapshot.as_raw_handle().cast(), &mut entry) };
        }
        false
    }

    pub fn prepare_pipe(_pipe: &impl AsRawHandle) -> io::Result<()> {
        Ok(())
    }

    pub fn drain_pipe(
        pipe: &mut (impl Read + AsRawHandle),
        output: &mut Vec<u8>,
    ) -> io::Result<(bool, usize)> {
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle().cast(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            return if error.raw_os_error()
                == Some(winapi::shared::winerror::ERROR_BROKEN_PIPE as i32)
            {
                Ok((true, 0))
            } else {
                Err(error)
            };
        }
        if available == 0 {
            return Ok((false, 0));
        }
        // Single reader, at most the currently buffered bytes: Read cannot await a writer.
        let mut buffer = [0; 65536];
        let limit = (available as usize).min(buffer.len());
        let read = pipe.read(&mut buffer[..limit])?;
        output.extend_from_slice(&buffer[..read]);
        Ok((read == 0, read))
    }
}

#[cfg(unix)]
mod native {
    use std::io::{self, Read};
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    pub struct ProcessGroup(i32);
    impl Drop for ProcessGroup {
        fn drop(&mut self) {
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
        }
    }

    pub fn spawn(command: &mut Command) -> Option<(Child, ProcessGroup)> {
        command.process_group(0);
        let child = command.spawn().ok()?;
        let group = ProcessGroup(child.id() as i32);
        Some((child, group))
    }

    pub fn prepare_pipe(pipe: &impl AsRawFd) -> io::Result<()> {
        let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
        if flags < 0
            || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn drain_pipe(pipe: &mut impl Read, output: &mut Vec<u8>) -> io::Result<(bool, usize)> {
        let mut buffer = [0; 65536];
        match pipe.read(&mut buffer) {
            Ok(read) => {
                output.extend_from_slice(&buffer[..read]);
                Ok((read == 0, read))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok((false, 0))
            }
            Err(error) => Err(error),
        }
    }
}

pub(super) use native::{drain_pipe, prepare_pipe, spawn};
