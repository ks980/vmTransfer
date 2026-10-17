//! ScreenCast D-Bus lifecycle. Subscribe before requesting to avoid fast-response
//! races. Never access a general PipeWire remote or persist capture permissions.
use super::portal::{CaptureFailure, CaptureTarget};
use futures_lite::StreamExt;
use std::{collections::HashMap, fs::File, io::Read, os::fd::OwnedFd};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection, Proxy,
};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
type Reply = HashMap<String, OwnedValue>;
type Options<'a> = HashMap<&'a str, Value<'a>>;
type Result<T> = std::result::Result<T, CaptureFailure>;

fn backend(error: impl std::fmt::Display) -> CaptureFailure {
    CaptureFailure::Backend(error.to_string())
}
fn bus_error(error: zbus::Error) -> CaptureFailure {
    if let zbus::Error::MethodError(name, _, _) = &error {
        if name.as_str().contains("AccessDenied") || name.as_str().contains("NotAllowed") {
            return CaptureFailure::PermissionDenied;
        }
        if name.as_str().contains("ServiceUnknown") || name.as_str().contains("UnknownInterface") {
            return CaptureFailure::PortalUnavailable;
        }
    }
    backend(error)
}
fn token() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(backend)?;
    Ok(format!(
        "vmt_{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}
fn request_path(connection: &Connection, token: &str) -> Result<String> {
    let sender = connection
        .unique_name()
        .ok_or(CaptureFailure::PortalUnavailable)?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    Ok(format!("{PATH}/request/{sender}/{token}"))
}
fn response(code: u32, fields: Reply) -> Result<Reply> {
    match code {
        0 => Ok(fields),
        1 => Err(CaptureFailure::Cancelled),
        _ => Err(CaptureFailure::Backend(
            "portal request failed (response 2); check GNOME portal permissions".into(),
        )),
    }
}

async fn request(
    connection: &Connection,
    session: Option<&OwnedObjectPath>,
    method: &str,
    mut options: Options<'_>,
) -> Result<Reply> {
    let handle = token()?;
    options.insert("handle_token", Value::from(handle.as_str()));
    let path = request_path(connection, &handle)?;
    let proxy = Proxy::new(
        connection,
        DEST,
        path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .await
    .map_err(bus_error)?;
    let mut signals = proxy.receive_signal("Response").await.map_err(bus_error)?;
    let reply = match session {
        None => {
            connection
                .call_method(Some(DEST), PATH, Some(SCREENCAST), method, &(options,))
                .await
        }
        Some(session) if method == "Start" => {
            connection
                .call_method(
                    Some(DEST),
                    PATH,
                    Some(SCREENCAST),
                    method,
                    &(session, "", options),
                )
                .await
        }
        Some(session) => {
            connection
                .call_method(
                    Some(DEST),
                    PATH,
                    Some(SCREENCAST),
                    method,
                    &(session, options),
                )
                .await
        }
    }
    .map_err(bus_error)?;
    let actual: OwnedObjectPath = reply.body().deserialize().map_err(backend)?;
    if actual.as_str() != path {
        let other = Proxy::new(connection, DEST, actual, "org.freedesktop.portal.Request")
            .await
            .map_err(bus_error)?;
        let _: std::result::Result<(), _> = other.call("Close", &()).await;
        return Err(backend(
            "portal returned an unexpected request handle; xdg-desktop-portal >= 0.9 required",
        ));
    }
    let message = signals.next().await.ok_or(CaptureFailure::Disconnected)?;
    let (code, fields): (u32, Reply) = message.body().deserialize().map_err(backend)?;
    response(code, fields)
}

/// Owns the connection/session until capture stops. Drop closes the Portal session
/// on every success/error path; no restore token or persisted permission is used.
pub(crate) struct PortalSession {
    pub connection: Connection,
    pub path: OwnedObjectPath,
    closed: bool,
}
impl PortalSession {
    pub async fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        let proxy = Proxy::new(
            &self.connection,
            DEST,
            self.path.clone(),
            "org.freedesktop.portal.Session",
        )
        .await
        .map_err(bus_error)?;
        let result: std::result::Result<(), _> = proxy.call("Close", &()).await;
        self.closed = true;
        match result {
            // The user/backend may have closed this object while we were hashing.
            Err(zbus::Error::MethodError(name, _, _))
                if matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.UnknownObject"
                        | "org.freedesktop.DBus.Error.UnknownMethod"
                ) =>
            {
                Ok(())
            }
            other => other.map_err(bus_error),
        }
    }
}
impl Drop for PortalSession {
    fn drop(&mut self) {
        let _ = futures_lite::future::block_on(self.close());
    }
}

pub(crate) struct AuthorizedRemote {
    pub session: PortalSession,
    pub fd: OwnedFd,
    pub node: u32,
    pub serial: Option<u64>,
    pub closed: zbus::proxy::SignalStream<'static>,
}
pub(crate) async fn authorize(target: CaptureTarget) -> Result<AuthorizedRemote> {
    let connection = Connection::session().await.map_err(|e| {
        backend(format!(
            "cannot connect to desktop session bus; run in your normal GNOME terminal: {e}"
        ))
    })?;
    let screencast = Proxy::new(&connection, DEST, PATH, SCREENCAST)
        .await
        .map_err(bus_error)?;
    let available: u32 = screencast
        .get_property("AvailableSourceTypes")
        .await
        .map_err(bus_error)?;
    let requested = match target {
        CaptureTarget::Window => 2,
        CaptureTarget::Monitor => 1,
        CaptureTarget::WindowOrMonitor => 3,
    };
    let types = requested & available;
    if types == 0 {
        return Err(backend(
            "requested window/monitor source is not supported by this portal",
        ));
    }
    let session_token = token()?;
    let mut options = Options::new();
    options.insert("session_handle_token", Value::from(session_token.as_str()));
    let mut created = request(&connection, None, "CreateSession", options).await?;
    let path = String::try_from(
        created
            .remove("session_handle")
            .ok_or_else(|| backend("missing session_handle"))?,
    )
    .map_err(backend)?;
    let mut session = PortalSession {
        connection: connection.clone(),
        path: OwnedObjectPath::try_from(path).map_err(backend)?,
        closed: false,
    };
    let session_proxy = Proxy::new(
        &connection,
        DEST,
        session.path.clone(),
        "org.freedesktop.portal.Session",
    )
    .await
    .map_err(bus_error)?;
    let closed = session_proxy
        .receive_signal("Closed")
        .await
        .map_err(bus_error)?;
    let mut options = Options::new();
    options.insert("types", Value::from(types));
    options.insert("multiple", Value::from(false));
    // Hidden is the protocol default; omit the key for old backends without this property.
    if let Ok(modes) = screencast.get_property::<u32>("AvailableCursorModes").await {
        if modes & 1 != 0 {
            options.insert("cursor_mode", Value::from(1u32));
        } else if modes & 4 != 0 {
            options.insert("cursor_mode", Value::from(4u32));
        }
    }
    request(&connection, Some(&session.path), "SelectSources", options).await?;
    let started = request(&connection, Some(&session.path), "Start", Options::new()).await?;
    let mut fields = started;
    let streams = fields
        .remove("streams")
        .ok_or_else(|| backend("missing PipeWire streams"))?;
    let mut streams: Vec<(u32, HashMap<String, OwnedValue>)> =
        Vec::try_from(streams).map_err(backend)?;
    if streams.len() != 1 {
        let _ = session.close().await;
        return Err(backend("portal must return exactly one selected stream"));
    }
    let (node, properties) = streams.remove(0);
    let serial = properties
        .get("pipewire-serial")
        .map(u64::try_from)
        .transpose()
        .map_err(backend)?;
    let reply = connection
        .call_method(
            Some(DEST),
            PATH,
            Some(SCREENCAST),
            "OpenPipeWireRemote",
            &(&session.path, Options::new()),
        )
        .await
        .map_err(bus_error)?;
    let fd: zbus::zvariant::OwnedFd = reply.body().deserialize().map_err(backend)?;
    Ok(AuthorizedRemote {
        session,
        fd: fd.into(),
        node,
        serial,
        closed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portal_response_distinguishes_cancellation_and_failure() {
        assert!(response(0, Reply::new()).is_ok());
        assert!(matches!(
            response(1, Reply::new()),
            Err(CaptureFailure::Cancelled)
        ));
        assert!(matches!(
            response(2, Reply::new()),
            Err(CaptureFailure::Backend(_))
        ));
        assert!(matches!(
            response(900, Reply::new()),
            Err(CaptureFailure::Backend(_))
        ));
    }
    #[test]
    fn request_tokens_are_path_safe_and_unique() {
        let a = token().unwrap();
        let b = token().unwrap();
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    }
}
