mod venue;
mod booking;
mod simulation;
mod config;
mod queue_manager;

use venue::{Sector, Venue};
use queue_manager::{SequentialQueueManager, QueueManager};

pub fn initialize_venue() -> Venue {
    let mut sectors: Vec<Sector> = vec![
        Sector::new("Standing", "ST", config::STANDING_CAPACITY, false),
        Sector::new("Golden Circle", "GC", config::GOLDEN_CIRCLE_CAPACITY, false),
    ];

    for i in 0..config::N_SECTORS {
        sectors.push(Sector::new(
            &format!("Seated {}", i + 1),
            &format!("S{}", i + 1),
            config::SEATED_SECTOR_CAPACITY,
            true,
        ));
    }

    Venue {
        name: String::from("Recinto de Ejemplo"),
        sectors,
    }
}

fn main() {
    let time_start = std::time::Instant::now();

    let venue = initialize_venue();

    let mut queue_manager = SequentialQueueManager::new(venue);
    let sold_tickets = queue_manager.run();

    let time_end = std::time::Instant::now();
    let duration = time_end.duration_since(time_start);
    let total_seconds = duration.as_secs_f32() * config::TIME_MULTIPLIER;
    let hours = (total_seconds / 3600.0).floor();
    let minutes = ((total_seconds % 3600.0) / 60.0).floor();
    let seconds = (total_seconds % 60.0).floor();
    println!("All seats booked in {:.0}:{:02.0}:{:02.0}", hours, minutes, seconds);

    let hours_float = total_seconds / 3600.0;
    if hours_float > 0.0 {
        println!(
            "Throughput: {:.2} tickets/hour",
            (queue_manager.get_venue().total_capacity() as f32) / hours_float
        );
    }
    println!("Total tickets sold: {}", sold_tickets.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_sequential_booking_correctness() {
        let sectors = vec![
            Sector::new("Standing", "ST", 50, false),
            Sector::new("Seated 1", "S1", 30, true),
            Sector::new("Seated 2", "S2", 20, true),
        ];
        let venue = Venue {
            name: "Small Test Arena".to_string(),
            sectors,
        };
        let expected_capacity = venue.total_capacity();

        let mut queue_manager = SequentialQueueManager::new(venue);
        let sold_tickets = queue_manager.run();

        // 1. All tickets must be sold
        assert_eq!(
            sold_tickets.len(),
            expected_capacity,
            "Total sold tickets must match venue capacity"
        );

        // 2. No duplicate ticket IDs
        let mut unique_set = HashSet::new();
        for id in &sold_tickets {
            assert!(
                unique_set.insert(id.clone()),
                "Duplicate ticket ID found: {}",
                id
            );
        }

        // 3. Venue state consistency
        let final_venue = queue_manager.get_venue();
        assert_eq!(final_venue.total_booked_seats(), expected_capacity);
        assert_eq!(final_venue.total_available_seats(), 0);
    }
}
