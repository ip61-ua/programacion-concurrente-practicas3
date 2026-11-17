use std::sync::Arc;
use std::thread;
use crate::venue::Venue;
use crate::booking::{SequentialBooker, ConcurrentBooker, SharedVenueState, Booker};
use crate::simulation::simulate_booking;

/// Trait que define las operaciones del gestor de colas.
pub trait QueueManager {
    fn new(venue: Venue) -> Self;
    fn run(&mut self) -> Vec<String>;
    fn get_venue(&self) -> Venue;
}

// ============================================================
// Implementación secuencial de referencia
// ============================================================

/// Implementación secuencial de referencia.
pub struct SequentialQueueManager {
    booker: SequentialBooker,
}

impl QueueManager for SequentialQueueManager {
    fn new(venue: Venue) -> Self {
        SequentialQueueManager {
            booker: SequentialBooker::new(venue),
        }
    }

    fn run(&mut self) -> Vec<String> {
        let mut sold_tickets = Vec::with_capacity(self.booker.get_total_capacity());
        while self.booker.remaining_seats() > 0 {
            let tickets = simulate_booking(&mut self.booker);
            sold_tickets.extend(tickets);
        }
        sold_tickets
    }

    fn get_venue(&self) -> Venue {
        self.booker.get_venue()
    }
}

// ============================================================
// Implementación concurrente — recolección local por hilo
// ============================================================

/// Número de hilos workers.
///
/// Se usa `available_parallelism` × 32 para compensar el alto ratio sleep/work
/// de las sesiones de compra (los hilos pasan la mayor parte del tiempo durmiendo
/// en `random_time_delay` mientras los lock-holders ejecutan operaciones de ~µs).
/// Se acota entre 32 y 1024 para evitar overhead excesivo.
fn optimal_thread_count() -> usize {
    let cores = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // Con delays de ~1ms y operaciones de ~µs, ratio sleep/work ≈ 1000:1.
    // Multiplicador de 32 para saturar mejor el pipeline de ventas.
    (cores * 32).clamp(32, 1024)
}

/// Implementación concurrente del gestor de colas.
///
/// Lanza `N` hilos workers, cada uno con su propio `ConcurrentBooker` que
/// comparte el `Arc<SharedVenueState>` subyacente. Los workers ejecutan sesiones
/// de compra en bucle hasta que `remaining_seats() == 0`.
///
/// **Mejora clave: recolección local por hilo.** Cada worker acumula sus tickets
/// en un `Vec<String>` local (sin lock), y los vectores se fusionan al final
/// con un único paso secuencial tras el `join`. Esto elimina completamente la
/// contención del `Mutex<Vec<String>>` global que tenía la versión anterior.
pub struct ConcurrentQueueManager {
    /// Estado compartido del recinto (Arc para compartir entre hilos).
    state: Arc<SharedVenueState>,
}

impl QueueManager for ConcurrentQueueManager {
    fn new(venue: Venue) -> Self {
        let state = Arc::new(SharedVenueState::from_venue(&venue));
        ConcurrentQueueManager { state }
    }

    /// Orquesta la venta concurrente de todas las entradas del recinto.
    ///
    /// # Estrategia
    /// - Lanza `N = cores × 32` hilos workers (ver `optimal_thread_count`).
    /// - Cada worker tiene su propio `ConcurrentBooker` que comparte el estado vía `Arc`.
    /// - El worker ejecuta `simulate_booking` en bucle hasta que `remaining_seats() == 0`.
    /// - Los tickets vendidos se acumulan en un `Vec<String>` **local** al hilo (zero contention).
    /// - Cada worker devuelve su Vec local mediante `JoinHandle::join()`.
    /// - El hilo principal fusiona todos los vectores parciales en uno solo.
    ///
    /// # Correctitud
    /// - `select_seats` y `book_seats` están protegidos por el `Mutex` del sector:
    ///   garantía de que dos hilos nunca venden el mismo asiento.
    /// - `remaining_seats` usa `AtomicUsize::Relaxed`: suficiente para la condición
    ///   de parada (el peor caso es que algún worker haga una sesión extra que falla,
    ///   lo que es correcto por diseño del protocolo).
    /// - No hay riesgo de deadlock: cada operación adquiere como máximo un lock.
    fn run(&mut self) -> Vec<String> {
        let n_threads = optimal_thread_count();
        let total_capacity = self.state.total_capacity;

        let mut handles: Vec<thread::JoinHandle<Vec<String>>> = Vec::with_capacity(n_threads);

        for _ in 0..n_threads {
            let state = Arc::clone(&self.state);

            let handle = thread::spawn(move || {
                // Cada hilo tiene su propio ConcurrentBooker (barato: solo un Arc clone).
                let mut booker = ConcurrentBooker::new(state);
                // Vec LOCAL: sin ningún lock para acumular tickets.
                let mut local_sold = Vec::new();

                loop {
                    // Condición de parada: lectura atómica, sin lock.
                    if booker.remaining_seats() == 0 {
                        break;
                    }

                    let tickets = simulate_booking(&mut booker);

                    if !tickets.is_empty() {
                        local_sold.extend(tickets);
                    }
                }

                local_sold
            });

            handles.push(handle);
        }

        // Fusionar todos los vectores parciales.
        // Pre-reservamos la capacidad total para evitar reallocations.
        let mut sold_tickets = Vec::with_capacity(total_capacity);
        for handle in handles {
            let partial = handle.join().expect("Worker thread panicked");
            sold_tickets.extend(partial);
        }

        sold_tickets
    }

    /// Devuelve un snapshot del estado actual del recinto.
    ///
    /// Solo se llama desde `main.rs` tras `run()` para estadísticas finales.
    fn get_venue(&self) -> Venue {
        self.state.to_venue()
    }
}


// ============================================================
// Tests de correctitud para ConcurrentQueueManager
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::{Sector, Venue};
    use std::collections::HashSet;

    fn small_venue() -> Venue {
        Venue {
            name: "Test Arena".to_string(),
            sectors: vec![
                Sector::new("Standing", "ST", 50, false),
                Sector::new("Golden Circle", "GC", 30, false),
                Sector::new("Seated 1", "S1", 20, true),
                Sector::new("Seated 2", "S2", 20, true),
            ],
        }
    }

    #[test]
    fn test_concurrent_all_seats_sold() {
        let venue = small_venue();
        let expected_capacity = venue.total_capacity();
        let mut qm = ConcurrentQueueManager::new(venue);
        let sold = qm.run();

        assert_eq!(
            sold.len(),
            expected_capacity,
            "Se deben vender exactamente {} entradas, se vendieron {}",
            expected_capacity,
            sold.len()
        );
    }

    #[test]
    fn test_concurrent_no_duplicates() {
        let venue = small_venue();
        let mut qm = ConcurrentQueueManager::new(venue);
        let sold = qm.run();

        let unique: HashSet<&String> = sold.iter().collect();
        assert_eq!(
            unique.len(),
            sold.len(),
            "Se encontraron {} entradas duplicadas",
            sold.len() - unique.len()
        );
    }

    #[test]
    fn test_concurrent_venue_state_consistent() {
        let venue = small_venue();
        let expected = venue.total_capacity();
        let mut qm = ConcurrentQueueManager::new(venue);
        let _ = qm.run();

        let final_venue = qm.get_venue();
        assert_eq!(
            final_venue.total_booked_seats(),
            expected,
            "El recinto debe tener todos los asientos en Booked"
        );
        assert_eq!(
            final_venue.total_available_seats(),
            0,
            "No deben quedar asientos Available"
        );
    }

    #[test]
    fn test_concurrent_larger_venue() {
        // Recinto más grande para detectar race conditions con más contención
        let sectors: Vec<Sector> = {
            let mut s = vec![
                Sector::new("Standing", "ST", 500, false),
                Sector::new("Golden Circle", "GC", 200, false),
            ];
            for i in 0..10 {
                s.push(Sector::new(
                    &format!("Seated {}", i + 1),
                    &format!("S{}", i + 1),
                    100,
                    true,
                ));
            }
            s
        };
        let venue = Venue { name: "Large Arena".to_string(), sectors };
        let expected = venue.total_capacity();
        let mut qm = ConcurrentQueueManager::new(venue);
        let sold = qm.run();

        assert_eq!(sold.len(), expected);
        let unique: HashSet<&String> = sold.iter().collect();
        assert_eq!(unique.len(), sold.len());
    }
}
