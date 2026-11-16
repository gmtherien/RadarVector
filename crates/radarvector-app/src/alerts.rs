//! Desktop notifications (ALR-01): followed CIDs connecting, ATC opening in a favourite.

use radarvector_core::callsign::Resolved;
use radarvector_core::{NetworkSnapshot, SnapshotDiff};
use radarvector_data::StaticData;

use crate::settings::{Favorite, Settings};

/// Messages to notify for a diff (pure, testable).
pub fn messages(settings: &Settings, data: &StaticData, snap: &NetworkSnapshot, diff: &SnapshotDiff) -> Vec<String> {
    let mut out = Vec::new();
    if settings.alert_followed_cids {
        for (cid, cs) in diff.pilots_connected.iter().chain(&diff.stations_opened) {
            if settings.followed_cids.contains(cid) {
                out.push(format!("{cs} ({cid}) is online"));
            }
        }
    }
    if settings.alert_favorite_atc {
        for (_, cs) in &diff.stations_opened {
            let Some(st) = snap.station_by_callsign(cs) else {
                continue;
            };
            let hit = settings.favorites.iter().any(|f| match (f, &st.resolved) {
                (Favorite::Airport(a), Resolved::Airport { icao } | Resolved::Approach { icao, .. }) => a == icao,
                (Favorite::Fir(b), Resolved::Fir(t) | Resolved::Uir { firs: t, .. }) => {
                    t.iter().any(|x| &x.boundary_id == b || &x.fir_icao == b)
                }
                (Favorite::Country(p), Resolved::Airport { icao } | Resolved::Approach { icao, .. }) => {
                    icao.starts_with(p.as_str())
                }
                (Favorite::Country(p), Resolved::Fir(t) | Resolved::Uir { firs: t, .. }) => {
                    t.iter().any(|x| x.fir_icao.starts_with(p.as_str()))
                }
                _ => false,
            });
            if hit {
                let _ = data;
                out.push(format!("{cs} opened ({})", st.data.frequency));
            }
        }
    }
    out
}

pub fn check(settings: &Settings, data: &StaticData, snap: &NetworkSnapshot, diff: &SnapshotDiff) {
    if !settings.notifications || diff.is_empty() {
        return;
    }
    let msgs = messages(settings, data, snap, diff);
    if msgs.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let body = msgs.join("\n");
        if let Err(e) = notify_rust::Notification::new()
            .summary("RadarVector")
            .body(&body)
            .show()
        {
            tracing::warn!("notification failed: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use radarvector_feed::models::decode_data_feed;

    #[test]
    fn followed_and_favorites() {
        let data = StaticData::from_texts(
            "[Airports]\nCYUL|Montreal|45.47|-73.74|YUL|CZUL|0\n[FIRs]\nCZUL|Montreal|MTL|CZUL\n",
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"CZUL"},"geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}}]}"#,
        )
        .unwrap()
        .0;
        let feed = decode_data_feed(br#"{"controllers":[{"cid":5,"callsign":"CYUL_TWR","frequency":"119.900"},{"cid":6,"callsign":"MTL_CTR"}],"pilots":[{"cid":7,"callsign":"ACA1"}]}"#).unwrap();
        let snap = NetworkSnapshot::build(&data, feed);
        let diff = SnapshotDiff::between(&NetworkSnapshot::default(), &snap);
        let mut s = Settings {
            followed_cids: vec![7],
            ..Default::default()
        };
        s.favorites.push(Favorite::Airport("CYUL".into()));
        let m = messages(&s, &data, &snap, &diff);
        assert_eq!(m.len(), 2, "{m:?}");
        s.favorites = vec![Favorite::Fir("CZUL".into())];
        assert!(messages(&s, &data, &snap, &diff).iter().any(|x| x.contains("MTL_CTR")));
        s.alert_favorite_atc = false;
        s.alert_followed_cids = false;
        assert!(messages(&s, &data, &snap, &diff).is_empty());
    }
}
