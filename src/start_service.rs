use crate::services::{Service, ServiceStatus};
use crate::units::*;
use std::error::Error;
use std::io::Read;
use std::os::unix::net::UnixListener;
use std::sync::Arc;

use std::process::{Command, Stdio};

fn after_fork_child(srvc: &mut Service, sockets: &SocketTable, notify_socket_env_var: &str) {
    let pid = nix::unistd::getpid();
    //here we are in the child process. We need to close every file descriptor we dont need anymore after the exec

    // TODO maybe all fd's should be marked with FD_CLOEXEC when openend
    // and here we only unflag those that we want to keep?
    trace!("[FORK_CHILD] CLOSING FDS");

    for sock_unit in sockets.values() {
        if let UnitSpecialized::Socket(sock) = &sock_unit.specialized {
            if !srvc.socket_names.contains(&sock.name) {
                trace!("[FORK_CHILD] CLOSE FDS FOR SOCKET: {}", sock.name);
                for conf in &sock.sockets {
                    match &conf.fd {
                        Some(fd) => {
                            let fd: i32 = (**fd).as_raw_fd();
                            nix::unistd::close(fd).unwrap();
                            trace!("[FORK_CHILD] DO CLOSE FD: {}", fd);
                        }
                        None => {
                            //this should not happen but if it does its not too bad
                        }
                    }
                }
            } else {
                trace!("[FORK_CHILD] DONT CLOSE FDS");
            }
        }
    }

    // The following two lines do deadlock after fork and before exec... I would have loved to just use these
    // This has probably something to do with the global env_lock() that is being used in the std
    // std::env::set_var("LISTEN_FDS", format!("{}", srvc.file_descriptors.len()));
    // std::env::set_var("LISTEN_PID", format!("{}", pid));

    // so lets use some unsafe instead, and use the same libc::setenv that the std uses but we dont care about the lock
    // This is the only thread in this process that is still running so we dont need any lock

    // TODO Maybe it would be better to have a simple wrapper that we can exec with a few sensible args
    // 1. list filedescriptors to keep open (maybe not event that. FD handling can be done here probably?)
    // 2. at least the number of fds
    // 3. the actual executable that should be run + their args
    //
    // This wrapper then does:
    // 1. Maybe close and dup2 fds
    // 2. Set appropriate env variables
    // 3. exec the actual executable we are trying to start here

    // This is all just that complicated because systemd promises to pass the correct PID in the env-var LISTEN_PID...

    let mut num_fds = 0;
    let mut name_lists = Vec::new();

    let sockets_by_name = get_sockets_by_name(sockets);
    for sock_name in &srvc.socket_names {
        trace!("[FORK_CHILD] Counting fds for socket: {}", sock_name);
        match sockets_by_name.get(sock_name) {
            Some(sock) => {
                num_fds += sock.sockets.len();
                name_lists.push(sock.build_name_list());
            }
            None => warn!("Socket was specified that cannot be found: {}", sock_name),
        }
        //let sock: Vec<_> = sockets
        //    .iter()
        //    .map(|(_id, unit)| {
        //        if let UnitSpecialized::Socket(sock) = unit.specialized {
        //            Some(sock)
        //        } else {
        //            None
        //        }
        //    })
        //    .filter(|sock| match sock {
        //        Some(sock) => {
        //            if sock.name == *sock_name {
        //                true
        //            } else {
        //                false
        //            }
        //        }
        //        None => false,
        //    })
        //    .map(|x| x.unwrap())
        //    .collect();
        //if sock.len() == 1 {
        //    let sock = sock[0];
        //    num_fds += sock.sockets.len();
        //    name_lists.push(sock.build_name_list());
        //}
    }

    let pid_str = &format!("{}", pid);
    let fds_str = &format!("{}", num_fds);

    unsafe fn setenv(key: &str, value: &str) {
        let k = std::ffi::CString::new(key.as_bytes()).unwrap();
        let v = std::ffi::CString::new(value.as_bytes()).unwrap();

        libc::setenv(k.as_ptr(), v.as_ptr(), 1);
    }
    let full_name_list = name_lists.join(":");
    unsafe {
        setenv("LISTEN_FDS", fds_str);
    }
    unsafe {
        setenv("LISTEN_PID", pid_str);
    }
    unsafe {
        setenv("LISTEN_FDNAMES", &full_name_list);
    }
    unsafe {
        setenv("NOTIFY_SOCKET", notify_socket_env_var);
    }

    trace!(
        "[FORK_CHILD] pid: {}, ENV: LISTEN_PID: {}  LISTEN_FD: {}, LISTEN_FDNAMES: {}",
        pid,
        pid_str,
        fds_str,
        full_name_list
    );

    // no more logging after this point!
    // The filedescriptor used by the logger might have been duped to another
    // one and logging into that one would be.... bad
    // Hopefully the close() means that no old logs will get written to that filedescriptor

    // start at 3. 0,1,2 are stdin,stdout,stderr
    let file_desc_offset = 3;
    let mut fd_idx = 0;

    for sock_name in &srvc.socket_names {
        match sockets_by_name.get(sock_name) {
            Some(socket) => {
                for sock_conf in &socket.sockets {
                    let new_fd = file_desc_offset + fd_idx;
                    let old_fd = match &sock_conf.fd {
                        Some(fd) => fd.as_raw_fd(),
                        None => panic!("No fd found for socket conf"),
                    };
                    if new_fd as i32 != old_fd {
                        //ignore output. newfd might already be closed
                        let _ = nix::unistd::close(new_fd as i32);
                        nix::unistd::dup2(old_fd, new_fd as i32).unwrap();
                    }
                    fd_idx += 1;
                }
            }
            None => warn!("Socket was specified that cannot be found: {}", sock_name),
        }
    }

    let split: Vec<&str> = match &srvc.service_config {
        Some(conf) => conf.exec.split(' ').collect(),
        None => unreachable!(),
    };

    let cmd = std::ffi::CString::new(split[0]).unwrap();
    let mut args = Vec::new();
    for arg in &split[1..] {
        args.push(std::ffi::CString::new(*arg).unwrap());
    }

    match nix::unistd::execv(&cmd, &args) {
        Ok(_) => {
            eprintln!("execv returned Ok()... This should never happen");
        }
        Err(e) => {
            eprintln!("execv errored: {:?}", e);
        }
    }
}

fn after_fork_parent(
    srvc: &mut Service,
    service_table: ArcMutServiceTable,
    id: InternalId,
    name: String,
    child: i32,
    notify_socket_env_var: &std::path::Path,
) {
    srvc.pid = Some(child as u32);

    trace!(
        "[FORK_PARENT] Service: {} forked with pid: {}",
        name,
        srvc.pid.unwrap()
    );

    if let Some(conf) = &srvc.service_config {
        if let ServiceType::Notify = conf.srcv_type {
            let listener = match &srvc.notify_access_socket {
                None => {
                    if notify_socket_env_var.exists() {
                        std::fs::remove_file(notify_socket_env_var).unwrap();
                    }
                    let new_listener = Arc::new(UnixListener::bind(notify_socket_env_var).unwrap());
                    srvc.notify_access_socket.get_or_insert(new_listener)
                }
                Some(l) => l,
            };

            trace!(
                "[FORK_PARENT] Waiting for a notification on: {:?}",
                &notify_socket_env_var
            );

            let (mut stream, _addr) = listener.accept().unwrap();
            trace!("[FORK_PARENT] Got notification connection");

            loop {
                let bytes: Vec<_> = (&mut stream)
                    .bytes()
                    .map(std::result::Result::unwrap)
                    .take_while(|x| *x != b'\n')
                    .collect();
                let note_string = String::from_utf8(bytes).unwrap();
                trace!(
                    "[FORK_PARENT] Notification received from service: {:?}",
                    note_string,
                );
                crate::notification_handler::handle_notification_message(
                    &note_string,
                    srvc,
                    name.clone(),
                );
                if let ServiceStatus::Running = srvc.status {
                    break;
                } else {
                    trace!("[FORK_PARENT] Service still not ready",);
                }
            }
            crate::notification_handler::handle_stream(stream, id, service_table);
        } else {
            trace!("[FORK_PARENT] service {} doesnt notify", name);
            srvc.status = ServiceStatus::Running;
        }
    }
}

fn start_service_with_filedescriptors(
    srvc: &mut Service,
    service_table: ArcMutServiceTable,
    id: InternalId,
    name: String,
    sockets: &SocketTable,
) {
    // 1. fork
    // 2. in fork use dup2 to map all relevant file desrciptors to 3..x
    // 3. in fork mark all other file descriptors with FD_CLOEXEC
    // 4. set relevant env varibales $LISTEN_FDS $LISTEN_PID
    // 4. execve the cmd with the args

    // setup socket for notifications from the service
    let notify_dir_path = std::path::PathBuf::from("./notifications");
    if !notify_dir_path.exists() {
        std::fs::create_dir_all(&notify_dir_path).unwrap();
    }
    let daemon_socket_path = notify_dir_path.join(format!("{}.notifiy_socket", &name));

    // NOTIFY_SOCKET
    let notify_socket_env_var = if daemon_socket_path.starts_with(".") {
        let cur_dir = std::env::current_dir().unwrap();
        cur_dir.join(&daemon_socket_path)
    } else {
        daemon_socket_path
    };

    match nix::unistd::fork() {
        Ok(nix::unistd::ForkResult::Parent { child, .. }) => {
            after_fork_parent(
                srvc,
                service_table,
                id,
                name,
                child,
                std::path::Path::new(notify_socket_env_var.to_str().unwrap()),
            );
        }
        Ok(nix::unistd::ForkResult::Child) => {
            after_fork_child(srvc, sockets, notify_socket_env_var.to_str().unwrap());
        }
        Err(_) => println!("Fork for service: {} failed", name),
    }
}

pub fn start_service(
    srvc: &mut Service,
    name: String,
    sockets: &SocketTable,
    id: InternalId,
    service_table: ArcMutServiceTable,
) {
    srvc.status = ServiceStatus::Starting;

    let split: Vec<&str> = match &srvc.service_config {
        Some(conf) => conf.exec.split(' ').collect(),
        None => return,
    };

    if let Some(srvc_conf) = &srvc.service_config {
        if !srvc_conf.sockets.is_empty() {
            start_service_with_filedescriptors(srvc, service_table, id, name, sockets);
        } else {
            let mut cmd = Command::new(split[0]);
            for part in &split[1..] {
                cmd.arg(part);
            }

            cmd.stdout(Stdio::null());

            match cmd.spawn() {
                Ok(child) => {
                    srvc.pid = Some(child.id());
                    srvc.status = ServiceStatus::Running;

                    trace!("Service: {} started with pid: {}", name, srvc.pid.unwrap());
                }
                Err(e) => panic!(e.description().to_owned()),
            }
        }
    }
}