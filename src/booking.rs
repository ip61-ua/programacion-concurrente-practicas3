use std::sync::{Arc, Mutex};
use crate::venue::{self, SectorState};

/// Trait que define las operaciones del motor de reservas (*Booker*).
///
/// Modela la interfaz que utiliza la simulación (`simulation.rs`) para consultar
/// disponibilidad, reservar provisionalmente localidades y consolidar la compra.
pub trait Booker {
    /// Nombre del recinto actual.
    fn get_venue_name(&self) -> &str;

    /// Capacidad total acumulada de todos los sectores del recinto.
    fn get_total_capacity(&self) -> usize;

    /// Número total de localidades disponibles actualmente en el recinto.
    #[allow(dead_code)]
    fn get_total_available_seats(&self) -> usize;

    /// Lista de códigos de los sectores que aún disponen de plazas libres.
    fn get_available_sectors(&self) -> Vec<String>;

    /// Indica si un sector tiene asientos numerados (`true`) o es de admisión general (`false`).
    #[allow(dead_code)]
    fn is_sector_numbered(&self, sector_code: &str) -> bool;

    /// Lista de identificadores de localidades disponibles en el sector indicado.
    fn get_available_seats(&self, sector_code: &str) -> Vec<String>;

    /// Número de entradas que faltan por vender en todo el recinto.
    fn remaining_seats(&self) -> usize;

    /// Notifica que una sesión de compra va a interactuar con un sector.
    fn select_sector(&mut self, sector_code: &str);

    /// Notifica que una sesión de compra abandona el sector sin completar la compra.
    fn unselect_sector(&mut self, sector_code: &str);

    /// Cancela la reserva provisional de las localidades indicadas,
    /// devolviéndolas al estado `Available`.
    fn deselect_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String>;

    /// Bloquea temporalmente las localidades indicadas cambiando su estado a `Selected`.
    ///
    /// Retorna `Ok(())` si todas se han podido reservar provisionalmente o `Err` si alguna ya no estaba disponible.
    fn select_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String>;

    /// Confirma definitivamente la compra de las localidades indicadas pasando su estado a `Booked`.
    ///
    /// Retorna `Ok(Vec<String>)` con los identificadores únicos definitivos de las entradas vendidas
    /// o `Err` si la reserva provisional no era válida o expiró.
    fn book_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<Vec<String>, String>;

    /// Devuelve una copia/instantánea del recinto (`Venue`) con el estado de todos sus sectores.
    fn get_venue(&self) -> venue::Venue;
}

// ============================================================
// Implementación secuencial de referencia
// ============================================================

/// Implementación secuencial de referencia del trait `Booker`.
///
/// Modifica directamente la estructura `Venue` en memoria sin sincronización multihilo.
pub struct SequentialBooker {
    venue: venue::Venue,
}

impl SequentialBooker {
    pub fn new(venue: venue::Venue) -> Self {
        Self { venue }
    }
}

impl Booker for SequentialBooker {
    fn get_venue_name(&self) -> &str {
        &self.venue.name
    }

    fn get_total_capacity(&self) -> usize {
        self.venue.total_capacity()
    }

    fn get_total_available_seats(&self) -> usize {
        self.venue.total_available_seats()
    }

    fn get_available_sectors(&self) -> Vec<String> {
        self.venue
            .sectors
            .iter()
            .filter(|s| s.n_available_seats() > 0)
            .map(|s| s.code.clone())
            .collect()
    }

    fn is_sector_numbered(&self, sector_code: &str) -> bool {
        self.venue
            .sectors
            .iter()
            .find(|s| s.code == sector_code)
            .map(|s| s.is_numbered)
            .unwrap_or(false)
    }

    fn get_available_seats(&self, sector_code: &str) -> Vec<String> {
        if let Some(sector) = self.venue.sectors.iter().find(|s| s.code == sector_code) {
            sector.get_available_seats().iter().map(|s| s.id.clone()).collect()
        } else {
            panic!("Unknown sector: '{sector_code}'");
        }
    }

    fn remaining_seats(&self) -> usize {
        self.venue.total_capacity() - self.venue.total_booked_seats()
    }

    fn select_sector(&mut self, _sector_code: &str) {
        // En la versión secuencial básica no requiere acción adicional.
    }

    fn unselect_sector(&mut self, _sector_code: &str) {
        // En la versión secuencial básica no requiere acción adicional.
    }

    fn deselect_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let sector = self
            .venue
            .sectors
            .iter_mut()
            .find(|s| s.code == sector_code)
            .ok_or_else(|| format!("Unknown sector: '{sector_code}'"))?;

        // 1. Validación: verificar que todos los asientos existen y están en estado Selected
        for seat_id in seat_ids {
            match sector.seats.iter().find(|s| s.id == *seat_id) {
                Some(seat) if seat.state == venue::SeatState::Selected => {}
                Some(_) => return Err(format!("Seat {} is not selected", seat_id)),
                None => return Err(format!("Seat {} does not exist in sector {}", seat_id, sector_code)),
            }
        }

        // 2. Transición de estado: Selected -> Available
        for seat_id in seat_ids {
            if let Some(seat) = sector.seats.iter_mut().find(|s| s.id == *seat_id) {
                seat.state = venue::SeatState::Available;
            }
        }

        Ok(())
    }

    fn select_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let sector = self
            .venue
            .sectors
            .iter_mut()
            .find(|s| s.code == sector_code)
            .ok_or_else(|| format!("Unknown sector: '{sector_code}'"))?;

        // 1. Validación: verificar que todos los asientos existen y están en estado Available
        for seat_id in seat_ids {
            match sector.seats.iter().find(|s| s.id == *seat_id) {
                Some(seat) if seat.state == venue::SeatState::Available => {}
                Some(_) => return Err(format!("Seat {} is not available", seat_id)),
                None => return Err(format!("Seat {} does not exist in sector {}", seat_id, sector_code)),
            }
        }

        // 2. Transición de estado: Available -> Selected
        for seat_id in seat_ids {
            if let Some(seat) = sector.seats.iter_mut().find(|s| s.id == *seat_id) {
                seat.state = venue::SeatState::Selected;
            }
        }

        Ok(())
    }

    fn book_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<Vec<String>, String> {
        let sector = self
            .venue
            .sectors
            .iter_mut()
            .find(|s| s.code == sector_code)
            .ok_or_else(|| format!("Unknown sector: '{sector_code}'"))?;

        // 1. Validación: verificar que todos los asientos existen y están en estado Selected
        for seat_id in seat_ids {
            match sector.seats.iter().find(|s| s.id == *seat_id) {
                Some(seat) if seat.state == venue::SeatState::Selected => {}
                Some(seat) if seat.state == venue::SeatState::Booked => {
                    return Err(format!("Seat {} is already booked", seat_id));
                }
                Some(_) => {
                    return Err(format!("Seat {} must be selected before booking", seat_id));
                }
                None => return Err(format!("Seat {} does not exist in sector {}", seat_id, sector_code)),
            }
        }

        // 2. Transición de estado: Selected -> Booked
        for seat_id in seat_ids {
            if let Some(seat) = sector.seats.iter_mut().find(|s| s.id == *seat_id) {
                seat.state = venue::SeatState::Booked;
            }
        }

        Ok(seat_ids.to_vec())
    }

    fn get_venue(&self) -> venue::Venue {
        self.venue.clone()
    }
}

// ============================================================
// Estado compartido del recinto para uso concurrente
// ============================================================

/// Estado compartido entre todos los hilos compradores.
///
/// Contiene un `Mutex<SectorState>` por sector (grano fino) y un
/// contador atómico de asientos restantes para el hot-path del loop.
pub struct SharedVenueState {
    pub venue_name: String,
    pub total_capacity: usize,
    /// Un Mutex independiente por sector — operaciones en sectores distintos son completamente paralelas.
    pub sectors: Vec<Mutex<SectorState>>,
    /// Mapa de código de sector → índice en `sectors` para lookup O(1).
    pub sector_index: std::collections::HashMap<String, usize>,
    /// Contador atómico de asientos restantes (capacity - booked).
    /// Decrementado atómicamente al confirmar cada venta.
    pub remaining: std::sync::atomic::AtomicUsize,
}

impl SharedVenueState {
    /// Construye el estado compartido a partir del `Venue` secuencial.
    pub fn from_venue(venue: &venue::Venue) -> Self {
        let mut sectors = Vec::with_capacity(venue.sectors.len());
        let mut sector_index = std::collections::HashMap::with_capacity(venue.sectors.len());

        for (i, s) in venue.sectors.iter().enumerate() {
            sector_index.insert(s.code.clone(), i);
            sectors.push(Mutex::new(SectorState::from_sector(s)));
        }

        let total_capacity = venue.total_capacity();

        SharedVenueState {
            venue_name: venue.name.clone(),
            total_capacity,
            sectors,
            sector_index,
            remaining: std::sync::atomic::AtomicUsize::new(total_capacity),
        }
    }

    /// Busca el índice de un sector por su código. Panics si no existe.
    #[inline]
    pub fn sector_idx(&self, code: &str) -> usize {
        *self.sector_index.get(code).unwrap_or_else(|| panic!("Unknown sector: '{}'", code))
    }

    /// Reconstruye un `Venue` secuencial a partir del estado concurrente (snapshot).
    pub fn to_venue(&self) -> venue::Venue {
        let sectors = self
            .sectors
            .iter()
            .map(|m| m.lock().unwrap().to_sector())
            .collect();

        venue::Venue {
            name: self.venue_name.clone(),
            sectors,
        }
    }
}

// ============================================================
// ConcurrentBooker — implementación del trait Booker con interior mutability
// ============================================================

/// Implementación concurrente del trait `Booker`.
///
/// Cada hilo comprador tiene su propia instancia de `ConcurrentBooker`,
/// pero todos comparten el mismo `Arc<SharedVenueState>` subyacente.
/// Los métodos `&mut self` usan interior mutability (el estado real está detrás de Mutex).
pub struct ConcurrentBooker {
    pub state: Arc<SharedVenueState>,
}

impl ConcurrentBooker {
    pub fn new(state: Arc<SharedVenueState>) -> Self {
        ConcurrentBooker { state }
    }
}

impl Booker for ConcurrentBooker {
    fn get_venue_name(&self) -> &str {
        &self.state.venue_name
    }

    fn get_total_capacity(&self) -> usize {
        self.state.total_capacity
    }

    fn get_total_available_seats(&self) -> usize {
        self.state
            .sectors
            .iter()
            .map(|m| m.lock().unwrap().n_available())
            .sum()
    }

    /// Devuelve los códigos de sectores que aún tienen asientos disponibles.
    /// Adquiere cada lock brevemente para leer el contador.
    fn get_available_sectors(&self) -> Vec<String> {
        self.state
            .sectors
            .iter()
            .filter_map(|m| {
                let s = m.lock().unwrap();
                if s.n_available() > 0 {
                    Some(s.code.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn is_sector_numbered(&self, sector_code: &str) -> bool {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].lock().unwrap().is_numbered
    }

    /// Devuelve los IDs de asientos disponibles en el sector.
    /// Adquiere el lock del sector brevemente.
    fn get_available_seats(&self, sector_code: &str) -> Vec<String> {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].lock().unwrap().available_seat_ids()
    }

    /// Consulta atómica: no requiere ningún lock de sector.
    fn remaining_seats(&self) -> usize {
        self.state.remaining.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// No-op en la implementación concurrente: no hay estado por sesión que gestionar.
    fn select_sector(&mut self, _sector_code: &str) {}

    /// No-op en la implementación concurrente.
    fn unselect_sector(&mut self, _sector_code: &str) {}

    /// Libera la reserva provisional de los asientos indicados (Selected → Available).
    /// Adquiere el lock del sector durante la operación y lo libera al salir.
    fn deselect_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].lock().unwrap().deselect_seats(seat_ids)
    }

    /// Reserva provisional: Available → Selected.
    ///
    /// Si algún asiento ya no está disponible (fue tomado por otro hilo entre
    /// `get_available_seats` y `select_seats`), retorna `Err` sin modificar nada.
    /// El lock se mantiene durante toda la operación para garantizar atomicidad.
    fn select_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<(), String> {
        let idx = self.state.sector_idx(sector_code);
        self.state.sectors[idx].lock().unwrap().select_seats(seat_ids)
    }

    /// Confirma la venta definitiva: Selected → Booked.
    ///
    /// Decrementa el contador atómico global `remaining` por el número de asientos vendidos.
    /// El lock del sector garantiza que no hay doble-venta.
    fn book_seats(&mut self, sector_code: &str, seat_ids: &[String]) -> Result<Vec<String>, String> {
        let idx = self.state.sector_idx(sector_code);
        let result = self.state.sectors[idx].lock().unwrap().book_seats(seat_ids);
        if let Ok(ref tickets) = result {
            self.state
                .remaining
                .fetch_sub(tickets.len(), std::sync::atomic::Ordering::Relaxed);
        }
        result
    }

    /// Devuelve un snapshot del recinto (adquiere todos los locks secuencialmente).
    fn get_venue(&self) -> venue::Venue {
        self.state.to_venue()
    }
}