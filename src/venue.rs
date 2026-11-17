use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

// ============================================================
// Modelos originales (requeridos por main.rs y SequentialBooker)
// ============================================================

/// Modelo de un recinto (*Venue*), compuesto por un nombre y múltiples sectores.
#[derive(Clone)]
pub struct Venue {
    pub name: String,
    pub sectors: Vec<Sector>,
}

impl Venue {
    /// Capacidad total sumando todos los sectores.
    pub fn total_capacity(&self) -> usize {
        self.sectors.iter().map(|s| s.capacity).sum()
    }

    /// Total de asientos disponibles sumando todos los sectores.
    pub fn total_available_seats(&self) -> usize {
        self.sectors.iter().map(|s| s.n_available_seats()).sum()
    }

    /// Total de asientos ya vendidos (*Booked*) sumando todos los sectores.
    pub fn total_booked_seats(&self) -> usize {
        self.sectors.iter().map(|s| s.n_booked_seats()).sum()
    }
}

impl fmt::Display for Venue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}/{}\n",
            self.name,
            self.total_booked_seats(),
            self.total_capacity(),
        )?;

        write!(f, "Sectors:\n")?;

        for sector in &self.sectors {
            write!(
                f,
                "  - {} ({}) {}/{}\n",
                sector.name,
                sector.code,
                sector.n_booked_seats(),
                sector.capacity,
            )?;
        }
        Ok(())
    }
}

/// Representa un sector dentro del recinto.
#[derive(Clone)]
pub struct Sector {
    pub name: String,
    pub code: String,
    pub seats: Vec<Seat>,
    pub capacity: usize,
    pub is_numbered: bool,
}

/// Estado en el que se encuentra un asiento o localidad.
#[derive(PartialEq, Clone)]
pub enum SeatState {
    /// Asiento libre, disponible para selección.
    Available,
    /// Asiento bloqueado provisionalmente por una sesión de compra.
    Selected,
    /// Asiento vendido definitivamente.
    Booked,
}

/// Representa una localidad individual con su estado e identificador único.
#[derive(Clone)]
pub struct Seat {
    pub state: SeatState,
    pub id: String,
}

impl Sector {
    /// Crea un nuevo sector inicializando todos sus asientos en estado `Available`.
    pub fn new(name: &str, code: &str, capacity: usize, is_numbered: bool) -> Sector {
        let seats = (1..=capacity)
            .map(|i| Seat {
                state: SeatState::Available,
                id: format!("{}-{}", code, i),
            })
            .collect();

        Sector {
            name: name.to_string(),
            code: code.to_string(),
            seats,
            capacity,
            is_numbered,
        }
    }

    /// Devuelve referencias a todos los asientos que se encuentran actualmente disponibles.
    pub fn get_available_seats(&self) -> Vec<&Seat> {
        self.seats.iter().filter(|s| s.state == SeatState::Available).collect()
    }

    /// Devuelve el número de asientos actualmente en estado `Available`.
    pub fn n_available_seats(&self) -> usize {
        self.seats.iter().filter(|s| s.state == SeatState::Available).count()
    }

    /// Devuelve el número de asientos actualmente en estado `Booked`.
    pub fn n_booked_seats(&self) -> usize {
        self.seats.iter().filter(|s| s.state == SeatState::Booked).count()
    }
}

// ============================================================
// Constantes de estado para los AtomicU8 por asiento
// ============================================================

/// Estado "libre" — el asiento puede ser seleccionado por cualquier hilo.
pub const SEAT_AVAILABLE: u8 = 0;
/// Estado "bloqueado provisionalmente" — sólo el hilo propietario puede liberarlo o confirmarlo.
pub const SEAT_SELECTED: u8 = 1;
/// Estado "vendido definitivamente" — estado terminal, nunca revierte.
pub const SEAT_BOOKED: u8 = 2;

// ============================================================
// ConcurrentSector — estado por sector completamente lock-free con CAS
// ============================================================

/// Estado concurrente de un sector usando instrucciones CAS sobre `AtomicU8`.
///
/// **Sin ningún Mutex**: todos los accesos al estado de los asientos son
/// operaciones atómicas de bajo nivel. Múltiples hilos pueden operar sobre
/// el mismo sector concurrentemente sin esperas (wait-free en el caso exitoso,
/// lock-free en general).
///
/// # Invariantes CAS
/// - `select_seats`: `compare_exchange(AVAILABLE → SELECTED)` atómico por asiento.
///   Si algún CAS falla, se hace rollback atómico de los ya realizados.
/// - `book_seats`: `compare_exchange(SELECTED → BOOKED)` atómico por asiento.
///   Si algún CAS falla, se hace rollback a `SELECTED`.
/// - `deselect_seats`: `compare_exchange(SELECTED → AVAILABLE)` atómico por asiento.
pub struct ConcurrentSector {
    /// Nombre descriptivo del sector.
    pub name: String,
    /// Código corto del sector (ej. "ST", "GC", "S1").
    pub code: String,
    /// ¿Los asientos son numerados?
    pub is_numbered: bool,
    /// IDs de todos los asientos (inmutables tras construcción).
    pub seat_ids: Vec<String>,
    /// Mapa seat_id → índice en O(1). Inmutable tras construcción — sin sincronización.
    seat_id_to_idx: HashMap<String, usize>,
    /// Estado atómico de cada asiento: `SEAT_AVAILABLE` / `SEAT_SELECTED` / `SEAT_BOOKED`.
    ///
    /// La transición se realiza mediante `compare_exchange` (CAS), garantizando
    /// que sólo un hilo puede cambiar el estado de un asiento en cada momento.
    seat_states: Vec<AtomicU8>,
    /// Capacidad total del sector.
    pub capacity: usize,
    /// Contador atómico aproximado de asientos disponibles.
    /// Usado para filtrar rápidamente sectores sin recorrer `seat_states`.
    pub n_available: AtomicUsize,
    /// Contador atómico de asientos vendidos definitivamente.
    pub n_booked: AtomicUsize,
}

// SAFETY: Vec<AtomicU8> y AtomicUsize son Send+Sync. HashMap con claves/valores
// inmutables tras construcción también es Send+Sync.
unsafe impl Send for ConcurrentSector {}
unsafe impl Sync for ConcurrentSector {}

impl ConcurrentSector {
    /// Construye el estado concurrente de un sector a partir del modelo secuencial.
    pub fn from_sector(sector: &Sector) -> Self {
        let n = sector.seats.len();
        let seat_ids: Vec<String> = sector.seats.iter().map(|s| s.id.clone()).collect();

        // Construir HashMap inmutable para lookup O(1).
        let seat_id_to_idx: HashMap<String, usize> = seat_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();

        // Un AtomicU8 por asiento, todos inicializados a AVAILABLE.
        let seat_states: Vec<AtomicU8> = (0..n).map(|_| AtomicU8::new(SEAT_AVAILABLE)).collect();

        ConcurrentSector {
            name: sector.name.clone(),
            code: sector.code.clone(),
            is_numbered: sector.is_numbered,
            capacity: n,
            seat_ids,
            seat_id_to_idx,
            seat_states,
            n_available: AtomicUsize::new(n),
            n_booked: AtomicUsize::new(0),
        }
    }

    /// Número de asientos disponibles (contador atómico aproximado).
    #[inline]
    #[allow(dead_code)]
    pub fn n_available(&self) -> usize {
        self.n_available.load(Ordering::Relaxed)
    }

    /// Busca el índice interno de un seat_id en O(1) mediante el HashMap inmutable.
    #[inline]
    fn lookup(&self, seat_id: &str) -> Result<usize, String> {
        self.seat_id_to_idx
            .get(seat_id)
            .copied()
            .ok_or_else(|| format!("Seat {} not found in sector {}", seat_id, self.code))
    }

    /// Devuelve los IDs de todos los asientos actualmente disponibles.
    ///
    /// Recorre `seat_states` con cargas atómicas `Acquire` — sin lock.
    /// O(capacity), pero completamente libre de contención.
    pub fn available_seat_ids(&self) -> Vec<String> {
        let approx = self.n_available.load(Ordering::Relaxed);
        let mut available = Vec::with_capacity(approx);
        for (i, s) in self.seat_states.iter().enumerate() {
            if s.load(Ordering::Acquire) == SEAT_AVAILABLE {
                available.push(self.seat_ids[i].clone());
            }
        }
        available
    }

    /// Reserva provisional: `Available → Selected` mediante CAS atómico por asiento.
    ///
    /// Si algún `compare_exchange` falla (el asiento fue tomado por otro hilo),
    /// se realiza rollback atómico de todos los CAS ya realizados, devolviendo
    /// los asientos al estado `Available`. Todo sin locks.
    ///
    /// Complejidad: O(k) amortizado, k = número de asientos solicitados.
    pub fn select_seats(&self, seat_ids: &[String]) -> Result<(), String> {
        let mut cas_done: Vec<usize> = Vec::with_capacity(seat_ids.len());

        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;

            match self.seat_states[idx].compare_exchange(
                SEAT_AVAILABLE,
                SEAT_SELECTED,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => cas_done.push(idx),
                Err(_) => {
                    // Rollback atómico: devolver todos los asientos ya seleccionados a Available.
                    for &i in &cas_done {
                        self.seat_states[i].store(SEAT_AVAILABLE, Ordering::Release);
                    }
                    return Err(format!("Seat {} is not available", seat_id));
                }
            }
        }

        // Actualizar el contador aproximado de disponibles.
        self.n_available.fetch_sub(seat_ids.len(), Ordering::Relaxed);
        Ok(())
    }

    /// Libera la reserva provisional: `Selected → Available` mediante CAS atómico.
    ///
    /// Complejidad: O(k).
    pub fn deselect_seats(&self, seat_ids: &[String]) -> Result<(), String> {
        // Validación previa: todos deben estar en SELECTED.
        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;
            if self.seat_states[idx].load(Ordering::Acquire) != SEAT_SELECTED {
                return Err(format!("Seat {} is not selected", seat_id));
            }
        }

        // Transición atómica Selected → Available mediante compare_exchange
        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;
            let _ = self.seat_states[idx].compare_exchange(
                SEAT_SELECTED,
                SEAT_AVAILABLE,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
        }
        self.n_available.fetch_add(seat_ids.len(), Ordering::Relaxed);
        Ok(())
    }

    /// Confirmación definitiva: `Selected → Booked` mediante CAS atómico por asiento.
    ///
    /// Si algún CAS falla (estado inconsistente), se hace rollback a `Selected`.
    /// Estado terminal: `Booked` nunca revierte.
    ///
    /// Complejidad: O(k).
    pub fn book_seats(&self, seat_ids: &[String]) -> Result<Vec<String>, String> {
        let mut cas_done: Vec<usize> = Vec::with_capacity(seat_ids.len());

        for seat_id in seat_ids {
            let idx = self.lookup(seat_id)?;

            match self.seat_states[idx].compare_exchange(
                SEAT_SELECTED,
                SEAT_BOOKED,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => cas_done.push(idx),
                Err(actual) => {
                    // Rollback: devolver los ya confirmados a Selected.
                    for &i in &cas_done {
                        self.seat_states[i].store(SEAT_SELECTED, Ordering::Release);
                    }
                    let msg = if actual == SEAT_BOOKED {
                        format!("Seat {} is already booked", seat_id)
                    } else {
                        format!("Seat {} must be selected before booking", seat_id)
                    };
                    return Err(msg);
                }
            }
        }

        self.n_booked.fetch_add(seat_ids.len(), Ordering::Relaxed);
        Ok(seat_ids.to_vec())
    }

    /// Reconstruye un `Sector` secuencial a partir del estado atómico (snapshot).
    pub fn to_sector(&self) -> Sector {
        let seats = self
            .seat_ids
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let state = match self.seat_states[i].load(Ordering::Acquire) {
                    SEAT_SELECTED => SeatState::Selected,
                    SEAT_BOOKED => SeatState::Booked,
                    _ => SeatState::Available,
                };
                Seat { id: id.clone(), state }
            })
            .collect();

        Sector {
            name: self.name.clone(),
            code: self.code.clone(),
            seats,
            capacity: self.capacity,
            is_numbered: self.is_numbered,
        }
    }
}
