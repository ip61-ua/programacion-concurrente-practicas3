use rand::{random_bool, seq::{IndexedRandom, SliceRandom}, Rng};
use crate::booking::Booker;
use crate::config;

/// Simula un retardo temporal aleatorio escalado por `TIME_MULTIPLIER`.
///
/// Modela el tiempo de procesamiento o decisión humana del usuario sin saturar la CPU
/// mediante una pausa con `std::thread::sleep`.
pub fn random_time_delay(min_seconds: usize, max_seconds: usize) {
    let delay_secs = rand::rng().random_range(min_seconds..max_seconds) as f32 / config::TIME_MULTIPLIER;
    std::thread::sleep(std::time::Duration::from_secs_f32(delay_secs));
}

/// Orquesta el flujo de interacción de una sesión de compra individual contra un `Booker`.
///
/// Modela el comportamiento del frontend/cliente web:
/// 1. Consulta los sectores disponibles y baraja aleatoriamente el orden de preferencia.
/// 2. Selecciona un sector o pasa al siguiente según decisión probabilística.
/// 3. Consulta las localidades disponibles en dicho sector.
/// 4. Intenta una reserva provisional (`select_seats`) para $N$ localidades aleatorias.
/// 5. Toma una decisión: 50% cambia de opinión y cancela (`deselect_seats`), o 50% confirma la compra.
/// 6. Checkout: invoca `book_seats`. Si se consolida con éxito, devuelve las entradas vendidas.
///    Si falla, sufre una penalización de retardo y prueba con el siguiente sector de su lista.
pub fn simulate_booking(booker: &mut dyn Booker) -> Vec<String> {
    let n_tickets = rand::rng().random_range(config::MIN_TICKETS..=config::MAX_TICKETS);
    let mut available_sectors = booker.get_available_sectors();
    available_sectors.shuffle(&mut rand::rng());

    for sector in available_sectors {
        if random_bool(0.5) {
            // El usuario decide descartar este sector y mirar otro
            random_time_delay(15, 20);
            continue;
        }

        random_time_delay(20, 45);
        booker.select_sector(&sector);

        random_time_delay(25, 150);
        let available_seats = booker.get_available_seats(&sector);

        if n_tickets > available_seats.len() {
            // Aforo insuficiente en este sector para las entradas deseadas
            booker.unselect_sector(&sector);
            continue;
        }
        random_time_delay(25, 250);

        // Selección provisional de N localidades
        let selected_seats: Vec<String> = available_seats
            .choose_multiple(&mut rand::rng(), n_tickets)
            .cloned()
            .collect();
        let Ok(()) = booker.select_seats(&sector, &selected_seats) else {
            booker.unselect_sector(&sector);
            continue; // La reserva provisional falló por conflicto, prueba con otro sector
        };

        random_time_delay(15, 250);

        if random_bool(0.5) {
            // El usuario cambia de opinión y cancela la reserva provisional
            booker.unselect_sector(&sector);
            let _ = booker.deselect_seats(&sector, &selected_seats);
            continue;
        } else {
            // El usuario confirma la compra
            booker.unselect_sector(&sector);
            random_time_delay(20, 100);
            let Ok(booked_seats) = booker.book_seats(&sector, &selected_seats) else {
                // La confirmación definitiva falló; penalización y reintento con otra preferencia
                random_time_delay(120, 600);
                continue;
            };
            return booked_seats;
        }
    }
    Vec::new()
}