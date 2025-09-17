use pico_de_gallo_lib::PicoDeGallo;
use std::time::Duration;
use tokio::time::interval;

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
    let mut ticker = interval(Duration::from_millis(250));

    for i in 0..10 {
        ticker.tick().await;
        print!("Pinging with {i}... ");
        let res = gallo.ping(i).await.unwrap();
        println!("got {res}!");
        assert_eq!(res, i);
    }
}
