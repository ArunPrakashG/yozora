//! Small helpers over zbus: property bags, method calls and change notifications.
//!
//! Backends re-read a whole snapshot when anything changes instead of tracking individual
//! property updates. That keeps each one simple, and the services involved are small.

use std::{collections::HashMap, time::Duration};

use anyhow::Context as _;
use futures_lite::{FutureExt, StreamExt};
use zbus::{
    Connection, MatchRule, MessageStream,
    zvariant::{DynamicDeserialize, DynamicType, OwnedObjectPath, OwnedValue},
};

pub type Props = HashMap<String, OwnedValue>;

pub fn get<T: TryFrom<OwnedValue>>(props: &Props, key: &str) -> Option<T> {
    T::try_from(props.get(key)?.try_clone().ok()?).ok()
}

pub fn string(props: &Props, key: &str) -> String {
    get::<String>(props, key).unwrap_or_default()
}

pub fn path(props: &Props, key: &str) -> Option<String> {
    get::<OwnedObjectPath>(props, key).map(|p| p.to_string()).filter(|p| p != "/")
}

pub fn paths(props: &Props, key: &str) -> Vec<String> {
    get::<Vec<OwnedObjectPath>>(props, key).unwrap_or_default().into_iter().map(|p| p.to_string()).collect()
}

pub async fn call<B, R>(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> anyhow::Result<R>
where
    B: serde::Serialize + DynamicType,
    R: for<'d> DynamicDeserialize<'d>,
{
    let reply = conn
        .call_method(Some(dest), path, Some(iface), method, body)
        .await
        .with_context(|| format!("{iface}.{method} on {dest}"))?;
    Ok(reply.body().deserialize::<R>()?)
}

/// Calls a method whose reply we don't need.
pub async fn invoke<B>(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> anyhow::Result<()>
where
    B: serde::Serialize + DynamicType,
{
    conn.call_method(Some(dest), path, Some(iface), method, body)
        .await
        .with_context(|| format!("{iface}.{method} on {dest}"))?;
    Ok(())
}

pub async fn get_all(conn: &Connection, dest: &str, path: &str, iface: &str) -> anyhow::Result<Props> {
    call(conn, dest, path, "org.freedesktop.DBus.Properties", "GetAll", &(iface,)).await
}

pub async fn set_prop(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    name: &str,
    value: zbus::zvariant::Value<'_>,
) -> anyhow::Result<()> {
    invoke(conn, dest, path, "org.freedesktop.DBus.Properties", "Set", &(iface, name, value)).await
}

pub async fn name_has_owner(conn: &Connection, name: &str) -> bool {
    call::<_, bool>(
        conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        &(name,),
    )
    .await
    .unwrap_or(false)
}

/// Signals matching `rule`.
pub async fn signals(conn: &Connection, rule: MatchRule<'static>) -> anyhow::Result<MessageStream> {
    Ok(MessageStream::for_match_rule(rule, conn, Some(256)).await?)
}

/// Every signal from `sender`.
pub async fn signals_from(conn: &Connection, sender: &str) -> anyhow::Result<MessageStream> {
    let rule = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(sender.to_string())?.build();
    Ok(MessageStream::for_match_rule(rule, conn, Some(256)).await?)
}

/// What woke a backend loop.
pub enum Wake<C> {
    Command(C),
    Changed,
    /// The command channel closed: the app is exiting.
    Quit,
}

/// Waits for a command or a signal. After a signal, keeps absorbing further signals for
/// `settle` so a burst of property changes costs one refresh.
pub async fn next<C, S>(cmds: &async_channel::Receiver<C>, stream: &mut S, settle: Duration) -> anyhow::Result<Wake<C>>
where
    S: futures_lite::Stream<Item = zbus::Result<zbus::Message>> + Unpin,
{
    let first = async {
        match cmds.recv().await {
            Ok(cmd) => Ok(Wake::Command(cmd)),
            Err(_) => Ok(Wake::Quit),
        }
    }
    .or(async {
        match stream.next().await {
            Some(Ok(_)) => Ok(Wake::Changed),
            Some(Err(e)) => Err(anyhow::Error::from(e)),
            None => Err(anyhow::anyhow!("D-Bus connection closed")),
        }
    })
    .await?;
    if let Wake::Changed = first {
        let deadline = async_io::Timer::after(settle);
        let drain = async { while let Some(Ok(_)) = stream.next().await {} };
        drain
            .or(async {
                deadline.await;
            })
            .await;
    }
    Ok(first)
}
