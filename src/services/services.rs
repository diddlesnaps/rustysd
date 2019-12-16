use super::start_service::*;
use std::collections::HashMap;
use std::error::Error;
use std::os::unix::io::RawFd;
use std::os::unix::net::UnixDatagram;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;

use crate::units::*;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ServiceStatus {
    NeverRan,
    Starting,
    Running,
    Stopped,
}

impl ToString for ServiceStatus {
    fn to_string(&self) -> String {
        match *self {
            ServiceStatus::NeverRan => "NeverRan".into(),
            ServiceStatus::Running => "Running".into(),
            ServiceStatus::Starting => "Starting".into(),
            ServiceStatus::Stopped => "Stopped".into(),
        }
    }
}

#[derive(Debug)]
pub struct ServiceRuntimeInfo {
    pub restarted: u64,
    pub up_since: Option<std::time::Instant>,
}

#[derive(Debug)]
pub struct Service {
    pub pid: Option<nix::unistd::Pid>,
    pub service_config: Option<ServiceConfig>,

    pub status: ServiceStatus,
    pub socket_names: Vec<String>,

    pub status_msgs: Vec<String>,

    pub runtime_info: ServiceRuntimeInfo,

    pub notifications: Option<Arc<Mutex<UnixDatagram>>>,
    pub stdout_dup: Option<(RawFd, RawFd)>,
    pub stderr_dup: Option<(RawFd, RawFd)>,
    pub notifications_buffer: String,
}

impl Service {
    pub fn start(
        &mut self,
        id: InternalId,
        name: &String,
        other_units: ArcMutUnitTable,
        pids: ArcMutPidTable,
        notification_socket_path: std::path::PathBuf,
        eventfds: &[RawFd],
    ) {
        trace!("Start service {}", name);

        match self.status {
            ServiceStatus::NeverRan | ServiceStatus::Stopped => {
                let mut socket_units = HashMap::new();
                let mut socket_ids = Vec::new();
                let mut other_units_locked = other_units.lock().unwrap();
                for unit in other_units_locked.values() {
                    if let UnitSpecialized::Socket(sock) = &unit.specialized {
                        socket_ids.push((unit.id, sock.name.clone()));
                    }
                }
                for (id, name) in &socket_ids {
                    let sock = other_units_locked.remove(&id).unwrap();
                    socket_units.insert(name, sock);
                }
                let mut sockets = HashMap::new();
                for (name, unit) in &socket_units {
                    if let UnitSpecialized::Socket(sock) = &unit.specialized {
                        let name: String = name.to_string();
                        sockets.insert(name, sock);
                    }
                }

                start_service(self, name.clone(), &sockets, notification_socket_path);

                for (id, name) in &socket_ids {
                    other_units_locked.insert(*id, socket_units.remove(&name).unwrap());
                }

                if let Some(new_pid) = self.pid {
                    {
                        let mut pids = pids.lock().unwrap();
                        pids.insert(new_pid, PidEntry::Service(id));
                    }
                    crate::notification_handler::notify_event_fds(&eventfds)
                } else {
                    // TODO dont even start services that require this one
                }
            }
            _ => error!(
                "Tried to start service {} after it was already running",
                name
            ),
        }
    }
}

pub fn kill_services(
    ids_to_kill: Vec<InternalId>,
    service_table: &mut ServiceTable,
    pid_table: &mut PidTable,
) {
    //TODO killall services that require this service
    for id in ids_to_kill {
        let srvc_unit = service_table.get_mut(&id).unwrap();
        if let UnitSpecialized::Service(srvc) = &srvc_unit.specialized {
            let split: Vec<&str> = match &srvc.service_config {
                Some(conf) => {
                    if conf.stop.is_empty() {
                        continue;
                    }
                    conf.stop.split(' ').collect()
                }
                None => continue,
            };

            let mut cmd = Command::new(split[0]);
            for part in &split[1..] {
                cmd.arg(part);
            }
            cmd.stdout(Stdio::null());

            match cmd.spawn() {
                Ok(child) => {
                    pid_table.insert(
                        nix::unistd::Pid::from_raw(child.id() as i32),
                        PidEntry::Stop(srvc_unit.id),
                    );
                    trace!(
                        "Stopped Service: {} with pid: {:?}",
                        srvc_unit.conf.name(),
                        srvc.pid
                    );
                }
                Err(e) => panic!(e.description().to_owned()),
            }
        }
    }
}

pub fn service_exit_handler(
    pid: nix::unistd::Pid,
    code: i32,
    unit_table: ArcMutServiceTable,
    pid_table: ArcMutPidTable,
    notification_socket_path: std::path::PathBuf,
) {
    let srvc_id = {
        let unit_table_locked = unit_table.lock().unwrap();
        let pid_table_locked = &mut *pid_table.lock().unwrap();
        *(match pid_table_locked.get(&pid) {
            Some(entry) => match entry {
                PidEntry::Service(id) => id,
                PidEntry::Stop(id) => {
                    trace!(
                        "Stop process for service: {} exited with code: {}",
                        unit_table_locked.get(id).unwrap().conf.name(),
                        code
                    );
                    pid_table_locked.remove(&pid);
                    return;
                }
            },
            None => {
                warn!("All spawned processes should have a pid entry");
                return;
            }
        })
    };

    let mut unit = {
        let unit_table_locked: &mut HashMap<_, _> = &mut unit_table.lock().unwrap();
        unit_table_locked.remove(&srvc_id).unwrap()
    };

    trace!(
        "Service with id: {}, name: {} pid: {} exited with code: {}",
        srvc_id,
        unit.conf.name(),
        pid,
        code
    );

    if let UnitSpecialized::Service(srvc) = &mut unit.specialized {
        srvc.status = ServiceStatus::Stopped;

        if let Some(conf) = &srvc.service_config {
            if conf.keep_alive {
                srvc.start(
                    srvc_id,
                    &unit.conf.name(),
                    unit_table.clone(),
                    pid_table,
                    notification_socket_path,
                    &Vec::new(),
                );
            } else {
                trace!(
                    "Killing all services requiring service with id {}: {:?}",
                    srvc_id,
                    unit.install.required_by
                );
                let pid_table_locked = &mut *pid_table.lock().unwrap();
                let unit_table_locked = &mut *unit_table.lock().unwrap();
                kill_services(
                    unit.install.required_by.clone(),
                    unit_table_locked,
                    pid_table_locked,
                );
            }
        }
    }

    let unit_table_locked: &mut HashMap<_, _> = &mut unit_table.lock().unwrap();
    unit_table_locked.insert(srvc_id, unit);
}
