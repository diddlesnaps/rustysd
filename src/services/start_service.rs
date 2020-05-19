use super::fork_child;
use crate::fd_store::FDStore;
use crate::services::RunCmdError;
use crate::services::Service;
use crate::units::ServiceConfig;

fn make_env_json(name: String, value: String) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("name".to_owned(), serde_json::Value::String(name));
    map.insert("value".to_owned(), serde_json::Value::String(value));
    serde_json::Value::Object(map)
}

fn start_service_with_filedescriptors(
    srvc: &mut Service,
    conf: &ServiceConfig,
    name: &str,
    fd_store: &FDStore,
) -> Result<(), RunCmdError> {
    // check if executable even exists
    let cmd = std::path::PathBuf::from(&conf.exec.cmd);
    if !cmd.exists() {
        error!(
            "The service {} specified an executable that does not exist: {:?}",
            name, &conf.exec.cmd
        );
        return Err(RunCmdError::SpawnError(
            conf.exec.cmd.clone(),
            format!("Executable does not exist"),
        ));
    }
    if !cmd.is_file() {
        error!(
            "The service {} specified an executable that is not a file: {:?}",
            name, &cmd
        );
        return Err(RunCmdError::SpawnError(
            conf.exec.cmd.clone(),
            format!("Executable does not exist (is a directory)"),
        ));
    }

    // 1. fork
    // 1. in fork use dup2 to map all relevant file desrciptors to 3..x
    // 1. in fork mark all other file descriptors with FD_CLOEXEC
    // 1. in fork set relevant env varibales $LISTEN_FDS $LISTEN_PID
    // 1. in fork execve the cmd with the args
    // 1. in parent set pid and return. Waiting will be done afterwards if necessary

    super::fork_os_specific::pre_fork_os_specific(conf).map_err(|e| RunCmdError::Generic(e))?;

    let mut fds = Vec::new();
    let mut fd_names = Vec::new();

    for socket in &conf.sockets {
        let sock_fds = fd_store
            .get_global(&socket.name)
            .unwrap()
            .iter()
            .map(|(_, _, fd)| fd.as_raw_fd())
            .collect::<Vec<_>>();

        let sock_names = fd_store
            .get_global(&socket.name)
            .unwrap()
            .iter()
            .map(|(_, name, _)| name.clone())
            .collect::<Vec<_>>();

        fds.extend(sock_fds);
        fd_names.extend(sock_names);
    }

    let notifications_path = {
        if let Some(p) = &srvc.notifications_path {
            p.to_str().unwrap().to_owned()
        } else {
            unreachable!();
        }
    };

    let full_name_list = fd_names.join(":");
    let fdnames_env = ("LISTEN_FDNAMES".to_owned(), full_name_list);
    let listenfds_env = ("LISTEN_FDS".to_owned(), fds.len().to_string());
    let notifysock_env = ("NOTIFY_SOCKET".to_owned(), notifications_path);

    let env_json = serde_json::Value::Array(vec![
        make_env_json(fdnames_env.0, fdnames_env.1),
        make_env_json(listenfds_env.0, listenfds_env.1),
        make_env_json(notifysock_env.0, notifysock_env.1),
    ]);

    let env_string = serde_json::to_string(&env_json).unwrap();
    let env_cstring = std::ffi::CString::new(env_string).unwrap();

    let conf_json = conf.export_json();
    let conf_string = serde_json::to_string(&conf_json).unwrap();
    let conf_cstring = std::ffi::CString::new(conf_string).unwrap();

    let args = &[
        "--conf".as_ptr() as *const i8,
        conf_cstring.as_ptr(),
        "--env".as_ptr() as *const i8,
        env_cstring.as_ptr(),
        "--command".as_ptr() as *const i8,
        "start".as_ptr() as *const i8,
        std::ptr::null(),
    ];
    let cmd_cstring = std::ffi::CString::new("rsdexec").unwrap();

    // make sure we have the lock that the child will need
    match nix::unistd::fork() {
        Ok(nix::unistd::ForkResult::Parent { child, .. }) => {
            srvc.pid = Some(child);
            srvc.process_group = Some(nix::unistd::Pid::from_raw(-child.as_raw()));
        }
        Ok(nix::unistd::ForkResult::Child) => {
            let stdout = {
                if let Some(stdio) = &srvc.stdout {
                    stdio.write_fd()
                } else {
                    unreachable!();
                }
            };
            let stderr = {
                if let Some(stdio) = &srvc.stderr {
                    stdio.write_fd()
                } else {
                    unreachable!();
                }
            };
            fork_child::after_fork_child(
                conf,
                &name,
                &fds,
                stdout,
                stderr,
                cmd_cstring.as_ptr(),
                args,
            );
        }
        Err(e) => error!("Fork for service: {} failed with: {}", name, e),
    }
    Ok(())
}

pub fn start_service(
    srvc: &mut Service,
    conf: &ServiceConfig,
    name: &str,
    fd_store: &FDStore,
) -> Result<(), super::RunCmdError> {
    start_service_with_filedescriptors(srvc, conf, name, fd_store)?;
    Ok(())
}
