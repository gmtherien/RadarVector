//! Minimal UI translations (English, French).

use std::sync::atomic::{AtomicU8, Ordering};

use crate::settings::Language;

static LANG: AtomicU8 = AtomicU8::new(0);

/// 0 = English, 1 = French.
pub fn set_language(l: Language) {
    let fr = match l {
        Language::French => true,
        Language::English => false,
        Language::System => system_is_french(),
    };
    LANG.store(u8::from(fr), Ordering::Relaxed);
}

fn system_is_french() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .or_else(macos_locale)
        .is_some_and(|v| v.to_ascii_lowercase().starts_with("fr"))
}

#[cfg(target_os = "macos")]
fn macos_locale() -> Option<String> {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

#[cfg(not(target_os = "macos"))]
fn macos_locale() -> Option<String> {
    None
}

macro_rules! strings {
    ($($key:ident => $en:expr, $fr:expr;)*) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms, dead_code)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum K { $($key,)* }
        pub fn t(k: K) -> &'static str {
            let fr = LANG.load(Ordering::Relaxed) == 1;
            match k { $(K::$key => if fr { $fr } else { $en },)* }
        }
    };
}

strings! {
    Map => "Map", "Carte";
    Controllers => "Controllers", "Contrôleurs";
    Flights => "Flights", "Vols";
    Servers => "Servers", "Serveurs";
    Prefiles => "Prefiles", "Plans déposés";
    Metar => "METAR", "METAR";
    Settings => "Settings", "Réglages";
    About => "About", "À propos";
    File => "File", "Fichier";
    View => "View", "Affichage";
    Tools => "Tools", "Outils";
    Help => "Help", "Aide";
    Quit => "Quit", "Quitter";
    Refresh => "Refresh", "Actualiser";
    Stop => "Stop", "Arrêter";
    AutoRefresh => "Auto refresh", "Actualisation auto";
    Every => "every", "toutes les";
    Seconds => "s", "s";
    Measure => "Measure", "Mesurer";
    Jump => "Jump to…", "Aller à…";
    Search => "Search (Ctrl+K)", "Rechercher (Ctrl+K)";
    Bookmarks => "Bookmarks", "Signets";
    AddBookmark => "Add bookmark…", "Ajouter un signet…";
    Favorites => "Favorites", "Favoris";
    AddFavorite => "Add to favorites", "Ajouter aux favoris";
    RemoveFavorite => "Remove from favorites", "Retirer des favoris";
    ConfirmAddFavorite => "Add this to your favorites?", "Ajouter aux favoris ?";
    Filters => "Filters", "Filtres";
    NewFilter => "New filter", "Nouveau filtre";
    EditFilters => "Edit filters…", "Modifier les filtres…";
    Name => "Name", "Nom";
    Callsign => "Callsign", "Indicatif";
    Frequency => "Frequency", "Fréquence";
    Rating => "Rating", "Qualification";
    Online => "Online", "En ligne";
    Aircraft => "Aircraft", "Appareil";
    Departure => "Departure", "Départ";
    Arrival => "Arrival", "Arrivée";
    Altitude => "Altitude", "Altitude";
    Groundspeed => "Ground speed", "Vitesse sol";
    Departures => "Departures", "Départs";
    Arrivals => "Arrivals", "Arrivées";
    Traffic => "Traffic in area", "Trafic dans la zone";
    Airports => "Airports", "Aéroports";
    Route => "Route", "Route";
    Remarks => "Remarks", "Remarques";
    Rules => "Rules", "Règles";
    Cruise => "Cruise", "Croisière";
    Alternate => "Alternate", "Dégagement";
    Squawk => "Squawk", "Transpondeur";
    AssignedSquawk => "Assigned squawk", "Code assigné";
    UpdateAvailable => "Update available", "Mise à jour disponible";
    UpToDate => "RadarVector is up to date", "RadarVector est à jour";
    Install => "Download and install", "Télécharger et installer";
    AutoCheckUpdates => "Check for updates at startup", "Rechercher les mises à jour au démarrage";
    Position => "Position", "Position";
    Logon => "Logon", "Connexion";
    Info => "Information", "Information";
    NoAtc => "No ATC online", "Aucun contrôleur en ligne";
    FilterText => "Filter…", "Filtrer…";
    Columns => "Columns", "Colonnes";
    Stations => "Stations", "Stations";
    Get => "Get", "Obtenir";
    RestoreDefaults => "Restore defaults", "Restaurer les défauts";
    Theme => "Theme", "Thème";
    Dark => "Dark", "Sombre";
    Light => "Light", "Clair";
    Language => "Language", "Langue";
    System => "System", "Système";
    Colors => "Colors", "Couleurs";
    FontScale => "Font size", "Taille du texte";
    InvertMouse => "Swap mouse buttons (pan with right button)", "Inverser les boutons de la souris (déplacer avec le bouton droit)";
    Projection => "Projection", "Projection";
    HideUnstaffed => "Hide unstaffed sectors", "Masquer les secteurs non dotés";
    ApproachRadius => "Approach circle radius (NM)", "Rayon du cercle d'approche (NM)";
    Labels => "Labels", "Étiquettes";
    Layers => "Layers", "Couches";
    Notifications => "Desktop notifications", "Notifications de bureau";
    FollowedCids => "Followed CIDs", "CID suivis";
    DataDir => "Local data folder (contributors)", "Dossier de données local (contributeurs)";
    UpdateData => "Update upstream data now", "Mettre à jour les données upstream";
    StaleData => "Data is %N min old", "Données vieilles de %N min";
    DownloadError => "Download failed", "Échec du téléchargement";
    NetworkStatus => "VATSIM network status", "État du réseau VATSIM";
    Clients => "clients", "clients";
    Unresolved => "unresolved", "non résolus";
    Close => "Close", "Fermer";
    Delete => "Delete", "Supprimer";
    Save => "Save", "Enregistrer";
    Cancel => "Cancel", "Annuler";
    Add => "Add", "Ajouter";
    Results => "Results", "Résultats";
    Center => "Center on map", "Centrer sur la carte";
    Details => "Details", "Fiche";
    CheckUpdates => "Check for updates", "Rechercher des mises à jour";
    Country => "Country", "Pays";
    Fir => "FIR", "FIR";
    Airport => "Airport", "Aéroport";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch() {
        set_language(Language::French);
        assert_eq!(t(K::Refresh), "Actualiser");
        set_language(Language::English);
        assert_eq!(t(K::Refresh), "Refresh");
    }
}
