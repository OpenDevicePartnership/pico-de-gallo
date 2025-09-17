use pico_de_gallo_lib::PicoDeGallo;

#[tokio::main]
pub async fn main() {
    let gallo = PicoDeGallo::new();

    tokio::select! {
        _ = gallo.wait_closed() => {
            println!("Client is closed, exiting...");
        }
        _ = run(&gallo) => {
            println!("App is done")
        }
    }
}

async fn run(gallo: &PicoDeGallo) {
    let mut high = 0;
    print!(
        r#"
   0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f
{:x} "#,
        high
    );

    for address in 0..=0x7f_u8 {
        match address {
            0x00..=0x07 | 0x78..=0x7f => {
                print!("RR ");
            }

            _ => {
                let result = gallo.i2c_read(address, 1).await;
                if result.is_ok() {
                    print!("{:02x} ", address);
                } else {
                    print!("-- ");
                }
            }
        }

        if address & 0x0f == 0x0f {
            high += 1;
            print!("\n");

            if high < 8 {
                print!("{:x} ", high);
            }
        }
    }
    print!("\n");
}
