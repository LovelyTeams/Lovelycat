use std::{
    io,
    path::Path,
    process::{Child, Command},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(windows)]
use std::os::windows::{
    io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::CommandExt,
};
#[cfg(windows)]
use windows_sys::Win32::System::{JobObjects::*, Threading::CREATE_NO_WINDOW};

pub(crate) fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

// The window owns this handle, while the worker retains only the flag.
pub(crate) struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub(crate) fn pair() -> (Self, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(false));
        (Self(flag.clone()), flag)
    }

    pub(crate) fn stop(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Drop for Cancel {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) struct ManagedChild {
    pub(crate) child: Child,
    #[cfg(windows)]
    job: Option<OwnedHandle>,
}

impl ManagedChild {
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::{mem::size_of_val, ptr::null};

            // Keep the handle uninherited so exiting Lovelycat closes the job.
            let raw_job = unsafe { CreateJobObjectW(null(), null()) };
            if raw_job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = unsafe { OwnedHandle::from_raw_handle(raw_job) };
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if unsafe {
                SetInformationJobObject(
                    raw_job,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of_val(&limits) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            let mut child = command.spawn()?;
            if unsafe { AssignProcessToJobObject(raw_job, child.as_raw_handle()) } == 0 {
                let error = io::Error::last_os_error();
                if !matches!(child.try_wait(), Ok(Some(_))) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
            Ok(Self {
                child,
                job: Some(job),
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                child: command.spawn()?,
            })
        }
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
