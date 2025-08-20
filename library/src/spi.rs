use crate::{Error, Result};
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use nusb::Interface;

#[allow(unused)]
pub(crate) struct Spi {
    writer: EndpointWrite<Bulk>,
    reader: EndpointRead<Bulk>,
}

impl Spi {
    pub(crate) fn new(interface: Interface) -> Result<Self> {
        let writer = interface
            .endpoint::<Bulk, Out>(0x02)
            .map_err(|_| Error::Io)?
            .writer(4096);
        let reader = interface
            .endpoint::<Bulk, In>(0x82)
            .map_err(|_| Error::Io)?
            .reader(4096);

        Ok(Self { writer, reader })
    }
}
