use std::time::Duration;

use bluer::agent::{Agent, AgentHandle, ReqResult, RequestConfirmation, RequestPinCode};
use bluer::rfcomm::{Profile, ProfileHandle, Role};
use bluer::{Adapter, Session, Uuid};
use ffsp_protocol::medium::{LaneChannel, LaneProfile, framed_channel};
use ffsp_protocol::wire::LaneId;
use futures_util::StreamExt;

use crate::error::{LinuxError, Result};

/// The RFCOMM service the phone connects to.
///
/// Android reaches a serial service by UUID rather than by channel number, so the channel is
/// left to BlueZ and published over SDP.
pub const SERVICE_UUID: Uuid = Uuid::from_u128(0x0000_FCF1_0000_1000_8000_0080_5F9B_34FB);

const PIN: &str = "000000";

/// The RFCOMM channel the service record points at.
///
/// Fixed rather than auto-assigned so the record below can name it, and so a client whose SDP
/// cache is stale can still be told a channel to dial.
pub const SERVICE_CHANNEL: u16 = 23;

/// The SDP record BlueZ publishes for the service.
///
/// Registering a profile under a UUID BlueZ does not recognise does not, on its own, put a
/// discoverable record in the database — and a client then gets nothing back from its SDP
/// search, which Android reports as `read failed, socket might closed or timeout`. Spelling
/// the record out is what makes the service findable.
fn service_record() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" ?>
<record>
  <attribute id="0x0001">
    <sequence><uuid value="{uuid}" /></sequence>
  </attribute>
  <attribute id="0x0004">
    <sequence>
      <sequence><uuid value="0x0100" /></sequence>
      <sequence><uuid value="0x0003" /><uint8 value="0x{channel:02x}" /></sequence>
    </sequence>
  </attribute>
  <attribute id="0x0005">
    <sequence><uuid value="0x1002" /></sequence>
  </attribute>
  <attribute id="0x0009">
    <sequence><sequence><uuid value="0x1101" /><uint16 value="0x0100" /></sequence></sequence>
  </attribute>
  <attribute id="0x0100"><text value="CrownOS FileShare" /></attribute>
</record>
"#,
        uuid = SERVICE_UUID,
        channel = SERVICE_CHANNEL,
    )
}

/// Bluetooth, brought up without anyone touching a settings pane.
///
/// The rig owns pairing: it registers an agent that accepts, makes the adapter discoverable, and
/// publishes the service. Everything a person would otherwise do in a Bluetooth menu happens
/// here, because a benchmark that needs a person between runs is not a benchmark.
#[derive(Debug)]
pub struct BluetoothMedium {
    adapter: Adapter,
    profile: ProfileHandle,
    _agent: AgentHandle,
}

impl BluetoothMedium {
    pub async fn open(discoverable_for: Duration) -> Result<Self> {
        let session = Session::new().await?;
        let agent = session.register_agent(auto_accepting_agent()).await?;
        let adapter = session.default_adapter().await?;

        adapter.set_powered(true).await?;
        adapter.set_pairable(true).await?;
        adapter
            .set_discoverable_timeout(discoverable_for.as_secs().try_into().unwrap_or(0))
            .await?;
        adapter.set_discoverable(true).await?;

        let profile = session
            .register_profile(Profile {
                uuid: SERVICE_UUID,
                name: Some("CrownOS FileShare".to_owned()),
                role: Some(Role::Server),
                channel: Some(SERVICE_CHANNEL),
                require_authentication: Some(false),
                require_authorization: Some(false),
                auto_connect: Some(true),
                service_record: Some(service_record()),
                ..Default::default()
            })
            .await?;

        Ok(Self {
            adapter,
            profile,
            _agent: agent,
        })
    }

    pub fn adapter_name(&self) -> &str {
        self.adapter.name()
    }

    pub async fn address(&self) -> Result<bluer::Address> {
        Ok(self.adapter.address().await?)
    }

    /// Waits for the phone to connect to the published service.
    ///
    /// This end only ever accepts: the phone dials out, because a phone cannot publish a
    /// service a laptop can find reliably. Nothing happens until the app is open, so the wait
    /// says so rather than sitting silent.
    pub async fn accept(&mut self) -> Result<(LaneChannel, bluer::Address)> {
        let request = loop {
            let waiting = tokio::time::sleep(std::time::Duration::from_secs(10));
            tokio::pin!(waiting);
            tokio::select! {
                request = self.profile.next() => {
                    break request.ok_or(LinuxError::NoBluetoothAdapter)?;
                }
                () = &mut waiting => {
                    tracing::info!(
                        "still waiting — open FileShare on the phone; this end only accepts"
                    );
                }
            }
        };
        let address = request.device();
        let stream = request.accept()?;
        Ok((
            framed_channel(LaneId::HANDSHAKE, LaneProfile::BLUETOOTH_CLASSIC, stream),
            address,
        ))
    }

    /// Whether this machine has a powered adapter at all.
    pub async fn is_available() -> bool {
        let Ok(session) = Session::new().await else {
            return false;
        };
        session.default_adapter().await.is_ok()
    }
}

/// An agent that says yes.
///
/// Both ends of this rig are the same person's devices, and a confirmation dialog on a laptop
/// that is being benchmarked over SSH is a dialog nobody sees. The pairing is still a real Noise
/// handshake above this; Bluetooth pairing here only gets the socket open.
fn auto_accepting_agent() -> Agent {
    Agent {
        request_default: true,
        request_pin_code: Some(Box::new(|_: RequestPinCode| {
            Box::pin(async move { ReqResult::Ok(PIN.to_owned()) })
        })),
        display_pin_code: Some(Box::new(|_| Box::pin(async move { ReqResult::Ok(()) }))),
        request_passkey: Some(Box::new(|_| Box::pin(async move { ReqResult::Ok(0) }))),
        display_passkey: Some(Box::new(|_| Box::pin(async move { ReqResult::Ok(()) }))),
        request_confirmation: Some(Box::new(|_: RequestConfirmation| {
            Box::pin(async move { ReqResult::Ok(()) })
        })),
        request_authorization: Some(Box::new(|_| Box::pin(async move { ReqResult::Ok(()) }))),
        authorize_service: Some(Box::new(|_| Box::pin(async move { ReqResult::Ok(()) }))),
        ..Default::default()
    }
}
