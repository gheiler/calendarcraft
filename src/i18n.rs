//! Interface languages: English, German, Spanish, French, Italian and Brazilian Portuguese (the
//! languages artcraftCloud's own interface has). `auto` follows the browser or the system.

use chrono::Weekday;
use std::cell::Cell;

pub static LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("de", "Deutsch"), ("es", "Español"), ("fr", "Français"), ("it", "Italiano"), ("pt-br", "Português (Brasil)")];

thread_local! {
    static LANG: Cell<usize> = const { Cell::new(0) };
}

pub fn language_name(code: &str) -> &'static str {
    LANGUAGES.iter().find(|(c, _)| *c == code).map(|(_, n)| *n).unwrap_or("English")
}

fn index_of(code: &str) -> Option<usize> {
    let code = code.trim().to_ascii_lowercase().replace('_', "-");
    LANGUAGES.iter().position(|(c, _)| *c == code).or_else(|| {
        let base = code.split('-').next().unwrap_or("");
        LANGUAGES.iter().position(|(c, _)| c.split('-').next() == Some(base))
    })
}

fn system_language() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.navigator().language()).unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("LC_ALL").or_else(|_| std::env::var("LANG")).unwrap_or_default().split('.').next().unwrap_or("").to_string()
    }
}

/// `auto` or a code; unknown codes fall back to English.
pub fn set_language(code: &str) {
    let idx = if code == "auto" || code.is_empty() { index_of(&system_language()) } else { index_of(code) };
    LANG.with(|l| l.set(idx.unwrap_or(0)));
}

pub fn current() -> &'static str {
    LANGUAGES[LANG.with(|l| l.get())].0
}

/// Translate an English interface text.
pub fn t(en: &'static str) -> &'static str {
    let i = LANG.with(|l| l.get());
    if i == 0 {
        return en;
    }
    TABLE.iter().find(|row| row[0] == en).map(|row| row[i]).unwrap_or(en)
}

const MONTHS: [[&str; 12]; 6] = [
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"],
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"],
    ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"],
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"],
];
const MONTHS_SHORT: [[&str; 12]; 6] = [
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."],
    ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"],
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
    ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
    ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"],
];
const DAYS: [[&str; 7]; 6] = [
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
    ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"],
    ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"],
    ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"],
    ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"],
    ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"],
];
const DAYS_SHORT: [[&str; 7]; 6] = [
    ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"],
    ["lun", "mar", "mié", "jue", "vie", "sáb", "dom"],
    ["lun", "mar", "mer", "jeu", "ven", "sam", "dim"],
    ["lun", "mar", "mer", "gio", "ven", "sab", "dom"],
    ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"],
];

fn li() -> usize {
    LANG.with(|l| l.get())
}

pub fn month_name(m: u32) -> &'static str {
    MONTHS[li()][(m.clamp(1, 12) - 1) as usize]
}
pub fn month_short(m: u32) -> &'static str {
    MONTHS_SHORT[li()][(m.clamp(1, 12) - 1) as usize]
}
pub fn weekday_name(d: Weekday) -> &'static str {
    DAYS[li()][d.num_days_from_monday() as usize]
}
pub fn weekday_short(d: Weekday) -> &'static str {
    DAYS_SHORT[li()][d.num_days_from_monday() as usize]
}
pub fn weekday_letter(d: Weekday) -> String {
    weekday_short(d).chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

// en, de, es, fr, it, pt-br
static TABLE: &[[&str; 6]] = &[
    ["Day", "Tag", "Día", "Jour", "Giorno", "Dia"],
    ["Week", "Woche", "Semana", "Semaine", "Settimana", "Semana"],
    ["Month", "Monat", "Mes", "Mois", "Mese", "Mês"],
    ["Quarter", "Quartal", "Trimestre", "Trimestre", "Trimestre", "Trimestre"],
    ["Year", "Jahr", "Año", "Année", "Anno", "Ano"],
    ["Today", "Heute", "Hoy", "Aujourd’hui", "Oggi", "Hoje"],
    ["Next", "Weiter", "Siguiente", "Suivant", "Avanti", "Próximo"],
    ["Previous", "Zurück", "Anterior", "Précédent", "Indietro", "Anterior"],
    ["New", "Neu", "Nuevo", "Nouveau", "Nuovo", "Novo"],
    ["New event", "Neuer Termin", "Nuevo evento", "Nouvel événement", "Nuovo evento", "Novo evento"],
    ["New event (⌘N)", "Neuer Termin (⌘N)", "Nuevo evento (⌘N)", "Nouvel événement (⌘N)", "Nuovo evento (⌘N)", "Novo evento (⌘N)"],
    ["Search", "Suchen", "Buscar", "Rechercher", "Cerca", "Buscar"],
    ["Settings", "Einstellungen", "Ajustes", "Réglages", "Impostazioni", "Configurações"],
    ["Show or hide the sidebar", "Seitenleiste ein- oder ausblenden", "Mostrar u ocultar la barra lateral", "Afficher ou masquer la barre latérale", "Mostra o nascondi la barra laterale", "Mostrar ou ocultar a barra lateral"],
    ["Lunch with Maya tomorrow at 12:30", "Mittagessen mit Maya morgen um 12:30", "Comida con Maya mañana a las 12:30", "Déjeuner avec Maya demain à 12h30", "Pranzo con Maya domani alle 12:30", "Almoço com Maya amanhã às 12:30"],
    ["Calendars", "Kalender", "Calendarios", "Calendriers", "Calendari", "Calendários"],
    ["Calendar", "Kalender", "Calendario", "Calendrier", "Calendario", "Calendário"],
    ["Add a calendar account", "Kalenderkonto hinzufügen", "Añadir una cuenta de calendario", "Ajouter un compte de calendrier", "Aggiungi un account calendario", "Adicionar uma conta de calendário"],
    ["Refresh", "Aktualisieren", "Actualizar", "Actualiser", "Aggiorna", "Atualizar"],
    ["No events", "Keine Termine", "Sin eventos", "Aucun événement", "Nessun evento", "Nenhum evento"],
    ["all-day", "ganztägig", "todo el día", "toute la journée", "tutto il giorno", "dia inteiro"],
    ["All-day", "Ganztägig", "Todo el día", "Toute la journée", "Tutto il giorno", "Dia inteiro"],
    ["continues", "geht weiter", "continúa", "suite", "continua", "continua"],
    ["more", "weitere", "más", "de plus", "altri", "mais"],
    ["events", "Termine", "eventos", "événements", "eventi", "eventos"],
    ["event", "Termin", "evento", "événement", "evento", "evento"],
    ["Added", "Hinzugefügt", "Añadido", "Ajouté", "Aggiunto", "Adicionado"],
    ["Saved", "Gesichert", "Guardado", "Enregistré", "Salvato", "Salvo"],
    ["Deleted", "Gelöscht", "Eliminado", "Supprimé", "Eliminato", "Excluído"],
    ["Title", "Titel", "Título", "Titre", "Titolo", "Título"],
    ["Close", "Schließen", "Cerrar", "Fermer", "Chiudi", "Fechar"],
    ["Choose…", "Auswählen …", "Elegir…", "Choisir…", "Scegli…", "Escolher…"],
    ["Date", "Datum", "Fecha", "Date", "Data", "Data"],
    ["Until", "Bis", "Hasta", "Jusqu’au", "Fino al", "Até"],
    ["same day", "gleicher Tag", "mismo día", "même jour", "stesso giorno", "mesmo dia"],
    ["Location", "Ort", "Ubicación", "Lieu", "Luogo", "Local"],
    ["Add location", "Ort hinzufügen", "Añadir ubicación", "Ajouter un lieu", "Aggiungi luogo", "Adicionar local"],
    ["Link", "Link", "Enlace", "Lien", "Link", "Link"],
    ["Alert", "Hinweis", "Aviso", "Alerte", "Avviso", "Alerta"],
    ["None", "Keiner", "Ninguno", "Aucune", "Nessuno", "Nenhum"],
    ["At time of event", "Zum Zeitpunkt des Termins", "A la hora del evento", "À l’heure de l’événement", "All’ora dell’evento", "Na hora do evento"],
    ["5 minutes before", "5 Minuten vorher", "5 minutos antes", "5 minutes avant", "5 minuti prima", "5 minutos antes"],
    ["10 minutes before", "10 Minuten vorher", "10 minutos antes", "10 minutes avant", "10 minuti prima", "10 minutos antes"],
    ["15 minutes before", "15 Minuten vorher", "15 minutos antes", "15 minutes avant", "15 minuti prima", "15 minutos antes"],
    ["30 minutes before", "30 Minuten vorher", "30 minutos antes", "30 minutes avant", "30 minuti prima", "30 minutos antes"],
    ["1 hour before", "1 Stunde vorher", "1 hora antes", "1 heure avant", "1 ora prima", "1 hora antes"],
    ["1 day before", "1 Tag vorher", "1 día antes", "1 jour avant", "1 giorno prima", "1 dia antes"],
    ["Repeat", "Wiederholen", "Repetir", "Répéter", "Ripeti", "Repetir"],
    ["Never", "Nie", "Nunca", "Jamais", "Mai", "Nunca"],
    ["Every day", "Täglich", "Cada día", "Chaque jour", "Ogni giorno", "Todo dia"],
    ["Every week", "Wöchentlich", "Cada semana", "Chaque semaine", "Ogni settimana", "Toda semana"],
    ["Every month", "Monatlich", "Cada mes", "Chaque mois", "Ogni mese", "Todo mês"],
    ["Every year", "Jährlich", "Cada año", "Chaque année", "Ogni anno", "Todo ano"],
    ["Custom", "Eigene", "Personalizado", "Personnalisé", "Personalizzato", "Personalizado"],
    ["repeats", "wiederholt sich", "se repite", "se répète", "si ripete", "repete"],
    ["Notes", "Notizen", "Notas", "Notes", "Note", "Notas"],
    ["Add notes", "Notizen hinzufügen", "Añadir notas", "Ajouter des notes", "Aggiungi note", "Adicionar notas"],
    ["Delete", "Löschen", "Eliminar", "Supprimer", "Elimina", "Excluir"],
    ["Delete event", "Termin löschen", "Eliminar evento", "Supprimer l’événement", "Elimina evento", "Excluir evento"],
    ["Keep", "Behalten", "Conservar", "Garder", "Mantieni", "Manter"],
    ["Delete event?", "Termin löschen?", "¿Eliminar el evento?", "Supprimer l’événement ?", "Eliminare l’evento?", "Excluir o evento?"],
    ["This can’t be undone.", "Das lässt sich nicht rückgängig machen.", "No se puede deshacer.", "Cette action est irréversible.", "L’operazione non può essere annullata.", "Isso não pode ser desfeito."],
    ["Untitled", "Ohne Titel", "Sin título", "Sans titre", "Senza titolo", "Sem título"],
    ["Details", "Details", "Detalles", "Détails", "Dettagli", "Detalhes"],
    ["Hide details", "Details ausblenden", "Ocultar detalles", "Masquer les détails", "Nascondi dettagli", "Ocultar detalhes"],
    ["Save", "Sichern", "Guardar", "Enregistrer", "Salva", "Salvar"],
    ["Cancel", "Abbrechen", "Cancelar", "Annuler", "Annulla", "Cancelar"],
    ["Add", "Hinzufügen", "Añadir", "Ajouter", "Aggiungi", "Adicionar"],
    ["Part of a repeating series with changes; edit it in the app that made it.", "Teil einer geänderten Serie; bearbeite sie in der App, die sie erstellt hat.", "Parte de una serie con cambios; edítala en la app que la creó.", "Fait partie d’une série modifiée ; modifiez-la dans l’app qui l’a créée.", "Parte di una serie con modifiche; modificala nell’app che l’ha creata.", "Parte de uma série com alterações; edite-a no app que a criou."],
    ["This calendar is read only.", "Dieser Kalender ist schreibgeschützt.", "Este calendario es de solo lectura.", "Ce calendrier est en lecture seule.", "Questo calendario è di sola lettura.", "Este calendário é somente leitura."],
    ["Type it: Dinner with Ada friday 7pm at Osteria", "Einfach tippen: Abendessen mit Ada Freitag 19 Uhr", "Escríbelo: Cena con Ada viernes 7pm", "Tapez : Dîner avec Ada vendredi 19h", "Scrivilo: Cena con Ada venerdì 19:00", "Digite: Jantar com Ada sexta 19h"],
    ["There is no calendar to add to. Add an account or a calendar in Settings.", "Es gibt keinen Kalender. Füge in den Einstellungen ein Konto oder einen Kalender hinzu.", "No hay ningún calendario. Añade una cuenta o un calendario en Ajustes.", "Aucun calendrier. Ajoutez un compte ou un calendrier dans Réglages.", "Nessun calendario. Aggiungi un account o un calendario in Impostazioni.", "Não há calendário. Adicione uma conta ou um calendário em Configurações."],
    ["Choose a calendar first", "Wähle zuerst einen Kalender", "Elige primero un calendario", "Choisissez d’abord un calendrier", "Scegli prima un calendario", "Escolha um calendário primeiro"],
    ["The date should look like 2026-11-24", "Das Datum sollte so aussehen: 2026-11-24", "La fecha debe tener la forma 2026-11-24", "La date doit ressembler à 2026-11-24", "La data deve essere come 2026-11-24", "A data deve ser como 2026-11-24"],
    ["The start time should look like 09:30", "Die Beginnzeit sollte so aussehen: 09:30", "La hora de inicio debe ser como 09:30", "L’heure de début doit ressembler à 09:30", "L’ora di inizio deve essere come 09:30", "A hora de início deve ser como 09:30"],
    ["The end time should look like 10:30", "Die Endzeit sollte so aussehen: 10:30", "La hora de fin debe ser como 10:30", "L’heure de fin doit ressembler à 10:30", "L’ora di fine deve essere come 10:30", "A hora de término deve ser como 10:30"],
    ["The event ends before it starts", "Der Termin endet, bevor er beginnt", "El evento termina antes de empezar", "L’événement se termine avant de commencer", "L’evento finisce prima di iniziare", "O evento termina antes de começar"],
    ["General", "Allgemein", "General", "Général", "Generale", "Geral"],
    ["Accounts", "Accounts", "Cuentas", "Comptes", "Account", "Contas"],
    ["Appearance", "Darstellung", "Apariencia", "Apparence", "Aspetto", "Aparência"],
    ["System", "System", "Sistema", "Système", "Sistema", "Sistema"],
    ["Light", "Hell", "Claro", "Clair", "Chiaro", "Claro"],
    ["Dark", "Dunkel", "Oscuro", "Sombre", "Scuro", "Escuro"],
    ["Language", "Sprache", "Idioma", "Langue", "Lingua", "Idioma"],
    ["Automatic", "Automatisch", "Automático", "Automatique", "Automatica", "Automático"],
    ["Animations", "Animationen", "Animaciones", "Animations", "Animazioni", "Animações"],
    ["On", "Ein", "Sí", "Oui", "Sì", "Sim"],
    ["Off", "Aus", "No", "Non", "No", "Não"],
    ["Default view", "Standardansicht", "Vista predeterminada", "Vue par défaut", "Vista predefinita", "Visualização padrão"],
    ["Day starts at", "Tag beginnt um", "El día empieza a las", "La journée commence à", "La giornata inizia alle", "O dia começa às"],
    ["New events go to", "Neue Termine in", "Nuevos eventos en", "Nouveaux événements dans", "Nuovi eventi in", "Novos eventos em"],
    ["CalDAV · provided by artcraftCloud", "CalDAV · von artcraftCloud", "CalDAV · de artcraftCloud", "CalDAV · fourni par artcraftCloud", "CalDAV · da artcraftCloud", "CalDAV · fornecido pelo artcraftCloud"],
    ["Subscription (read only)", "Abonnement (nur lesen)", "Suscripción (solo lectura)", "Abonnement (lecture seule)", "Abbonamento (sola lettura)", "Assinatura (somente leitura)"],
    ["Opened file", "Geöffnete Datei", "Archivo abierto", "Fichier ouvert", "File aperto", "Arquivo aberto"],
    ["File", "Datei", "Archivo", "Fichier", "File", "Arquivo"],
    ["On this device", "Auf diesem Gerät", "En este dispositivo", "Sur cet appareil", "Su questo dispositivo", "Neste dispositivo"],
    ["Remove", "Entfernen", "Quitar", "Retirer", "Rimuovi", "Remover"],
    ["Add an account", "Account hinzufügen", "Añadir una cuenta", "Ajouter un compte", "Aggiungi un account", "Adicionar uma conta"],
    ["CalDAV account", "CalDAV-Account", "Cuenta CalDAV", "Compte CalDAV", "Account CalDAV", "Conta CalDAV"],
    ["Subscription (.ics)", "Abonnement (.ics)", "Suscripción (.ics)", "Abonnement (.ics)", "Abbonamento (.ics)", "Assinatura (.ics)"],
    ["Name", "Name", "Nombre", "Nom", "Nome", "Nome"],
    ["Work", "Arbeit", "Trabajo", "Travail", "Lavoro", "Trabalho"],
    ["Address", "Adresse", "Dirección", "Adresse", "Indirizzo", "Endereço"],
    ["User name", "Benutzername", "Usuario", "Nom d’utilisateur", "Nome utente", "Usuário"],
    ["Password", "Passwort", "Contraseña", "Mot de passe", "Password", "Senha"],
    ["App password", "App-Passwort", "Contraseña de app", "Mot de passe d’app", "Password per app", "Senha de app"],
    ["Connect", "Verbinden", "Conectar", "Connecter", "Collega", "Conectar"],
    ["Enter the server address, starting with https://", "Gib die Serveradresse ein, beginnend mit https://", "Escribe la dirección del servidor, empezando por https://", "Saisissez l’adresse du serveur, commençant par https://", "Inserisci l’indirizzo del server, che inizia con https://", "Digite o endereço do servidor, começando com https://"],
    ["iCloud: https://caldav.icloud.com with an app-specific password. Fastmail: https://caldav.fastmail.com. Nextcloud: https://<server>/remote.php/dav. Google needs a sign-in CalendarCraft cannot do yet: add its secret iCal address as a subscription instead.", "iCloud: https://caldav.icloud.com mit einem app-spezifischen Passwort. Fastmail: https://caldav.fastmail.com. Nextcloud: https://<server>/remote.php/dav. Google braucht eine Anmeldung, die CalendarCraft noch nicht kann: füge stattdessen die geheime iCal-Adresse als Abonnement hinzu.", "iCloud: https://caldav.icloud.com con una contraseña específica de app. Fastmail: https://caldav.fastmail.com. Nextcloud: https://<servidor>/remote.php/dav. Google necesita un inicio de sesión que CalendarCraft aún no admite: añade su dirección iCal secreta como suscripción.", "iCloud : https://caldav.icloud.com avec un mot de passe d’app. Fastmail : https://caldav.fastmail.com. Nextcloud : https://<serveur>/remote.php/dav. Google demande une connexion que CalendarCraft ne gère pas encore : ajoutez son adresse iCal secrète comme abonnement.", "iCloud: https://caldav.icloud.com con una password specifica per app. Fastmail: https://caldav.fastmail.com. Nextcloud: https://<server>/remote.php/dav. Google richiede un accesso che CalendarCraft non gestisce ancora: aggiungi l’indirizzo iCal segreto come abbonamento.", "iCloud: https://caldav.icloud.com com uma senha específica de app. Fastmail: https://caldav.fastmail.com. Nextcloud: https://<servidor>/remote.php/dav. O Google exige um login que o CalendarCraft ainda não faz: adicione o endereço iCal secreto como assinatura."],
    ["In a plain browser page most servers refuse to answer (CORS). Inside artcraftCloud the requests go through its proxy.", "In einer normalen Browserseite antworten die meisten Server nicht (CORS). In artcraftCloud laufen die Anfragen über dessen Proxy.", "En una página normal del navegador la mayoría de servidores no responden (CORS). En artcraftCloud las peticiones pasan por su proxy.", "Dans une simple page web, la plupart des serveurs refusent de répondre (CORS). Dans artcraftCloud, les requêtes passent par son proxy.", "In una normale pagina del browser la maggior parte dei server non risponde (CORS). In artcraftCloud le richieste passano dal suo proxy.", "Numa página comum do navegador a maioria dos servidores não responde (CORS). No artcraftCloud as solicitações passam pelo proxy dele."],
    ["Calendar on this device", "Kalender auf diesem Gerät", "Calendario en este dispositivo", "Calendrier sur cet appareil", "Calendario su questo dispositivo", "Calendário neste dispositivo"],
    ["Personal", "Privat", "Personal", "Personnel", "Personale", "Pessoal"],
    ["Create", "Erstellen", "Crear", "Créer", "Crea", "Criar"],
    ["Connect an MCP client over stdio:", "Einen MCP-Client über stdio verbinden:", "Conecta un cliente MCP por stdio:", "Connectez un client MCP via stdio :", "Collega un client MCP via stdio:", "Conecte um cliente MCP via stdio:"],
    ["Try a request against this calendar:", "Eine Anfrage an diesen Kalender testen:", "Prueba una petición con este calendario:", "Essayez une requête sur ce calendrier :", "Prova una richiesta su questo calendario:", "Teste uma solicitação neste calendário:"],
    ["Run", "Ausführen", "Ejecutar", "Exécuter", "Esegui", "Executar"],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_switch_and_fall_back() {
        set_language("de");
        assert_eq!(t("Today"), "Heute");
        assert_eq!(month_name(3), "März");
        assert_eq!(t("not in the table"), "not in the table");
        set_language("pt-BR");
        assert_eq!(current(), "pt-br");
        set_language("xx");
        assert_eq!(t("Today"), "Today");
    }
}
