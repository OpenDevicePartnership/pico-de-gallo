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
    let version = gallo.version().await.unwrap();
    println!("Version: {:#?}", version);
}
