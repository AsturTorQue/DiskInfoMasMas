//! Interface languages. Each text has one entry per language, in the order of `Lang::ALL`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Lang {
    Es,
    En,
    Pt,
    Fr,
    De,
    It,
}

impl Lang {
    pub const ALL: [Lang; 6] = [Lang::Es, Lang::En, Lang::Pt, Lang::Fr, Lang::De, Lang::It];

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::Es => "Español",
            Lang::En => "English",
            Lang::Pt => "Português",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::It => "Italiano",
        }
    }

    fn index(self) -> usize {
        Lang::ALL.iter().position(|l| *l == self).unwrap_or(1)
    }

    /// The system language when it is one we have, English otherwise.
    pub fn from_system() -> Lang {
        let v = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .unwrap_or_default();
        match v.get(..2).unwrap_or("") {
            "es" => Lang::Es,
            "pt" => Lang::Pt,
            "fr" => Lang::Fr,
            "de" => Lang::De,
            "it" => Lang::It,
            _ => Lang::En,
        }
    }

    pub fn decimal_comma(self) -> bool {
        self != Lang::En
    }
}

impl Default for Lang {
    fn default() -> Self {
        Lang::from_system()
    }
}

//                 key                 Español, English, Português, Français, Deutsch, Italiano
const TEXTS: &[(&str, [&str; 6])] = &[
    ("menu_file", ["Archivo", "File", "Arquivo", "Fichier", "Datei", "File"]),
    ("quit", ["Salir", "Exit", "Sair", "Quitter", "Beenden", "Esci"]),
    ("menu_edit", ["Editar", "Edit", "Editar", "Édition", "Bearbeiten", "Modifica"]),
    ("copy_report", ["Copiar informe", "Copy report", "Copiar relatório", "Copier le rapport", "Bericht kopieren", "Copia rapporto"]),
    ("report_copied", ["Informe copiado al portapapeles", "Report copied to the clipboard", "Relatório copiado para a área de transferência", "Rapport copié dans le presse-papiers", "Bericht in die Zwischenablage kopiert", "Rapporto copiato negli appunti"]),
    ("menu_function", ["Función", "Function", "Função", "Fonction", "Funktion", "Funzione"]),
    ("refresh", ["Actualizar (F5)", "Refresh (F5)", "Atualizar (F5)", "Actualiser (F5)", "Aktualisieren (F5)", "Aggiorna (F5)"]),
    ("read_admin", ["Leer S.M.A.R.T. como administrador", "Read S.M.A.R.T. as administrator", "Ler S.M.A.R.T. como administrador", "Lire S.M.A.R.T. en administrateur", "S.M.A.R.T. als Administrator lesen", "Leggi S.M.A.R.T. come amministratore"]),
    ("auto_refresh", ["Actualización automática", "Auto refresh", "Atualização automática", "Actualisation automatique", "Automatisch aktualisieren", "Aggiornamento automatico"]),
    ("off", ["Desactivada", "Off", "Desativada", "Désactivée", "Aus", "Disattivato"]),
    ("thresholds_menu", ["Umbrales de aviso…", "Warning thresholds…", "Limites de aviso…", "Seuils d'alerte…", "Warnschwellen…", "Soglie di avviso…"]),
    ("menu_theme", ["Tema", "Theme", "Tema", "Thème", "Design", "Tema"]),
    ("green_mode", ["Modo verde", "Green mode", "Modo verde", "Mode vert", "Grüner Modus", "Modalità verde"]),
    ("fahrenheit", ["Grados Fahrenheit", "Fahrenheit degrees", "Graus Fahrenheit", "Degrés Fahrenheit", "Grad Fahrenheit", "Gradi Fahrenheit"]),
    ("theme_light", ["Claro", "Light", "Claro", "Clair", "Hell", "Chiaro"]),
    ("theme_dark", ["Oscuro", "Dark", "Escuro", "Sombre", "Dunkel", "Scuro"]),
    ("theme_mint", ["Menta", "Mint", "Menta", "Menthe", "Minze", "Menta"]),
    ("theme_ocean", ["Océano", "Ocean", "Oceano", "Océan", "Ozean", "Oceano"]),
    ("theme_sakura", ["Sakura", "Sakura", "Sakura", "Sakura", "Sakura", "Sakura"]),
    ("theme_contrast", ["Alto contraste", "High contrast", "Alto contraste", "Contraste élevé", "Hoher Kontrast", "Alto contrasto"]),
    ("menu_disk", ["Disco", "Disk", "Disco", "Disque", "Laufwerk", "Disco"]),
    ("menu_language", ["Idioma", "Language", "Idioma", "Langue", "Sprache", "Lingua"]),
    ("menu_help", ["Ayuda", "Help", "Ajuda", "Aide", "Hilfe", "Aiuto"]),
    ("about", ["Acerca de", "About", "Sobre", "À propos", "Über", "Informazioni"]),
    ("reading", ["Leyendo discos…", "Reading disks…", "Lendo discos…", "Lecture des disques…", "Laufwerke werden gelesen…", "Lettura dei dischi…"]),
    ("admin_banner", [
        "Para leer la salud (S.M.A.R.T.) de los discos hacen falta permisos de administrador.",
        "Reading disk health (S.M.A.R.T.) needs administrator rights.",
        "Para ler a saúde (S.M.A.R.T.) dos discos são necessárias permissões de administrador.",
        "La lecture de la santé (S.M.A.R.T.) des disques nécessite des droits d'administrateur.",
        "Zum Lesen des Laufwerkszustands (S.M.A.R.T.) sind Administratorrechte nötig.",
        "Per leggere la salute (S.M.A.R.T.) dei dischi servono i permessi di amministratore.",
    ]),
    ("read_smart_btn", ["Leer S.M.A.R.T.", "Read S.M.A.R.T.", "Ler S.M.A.R.T.", "Lire S.M.A.R.T.", "S.M.A.R.T. lesen", "Leggi S.M.A.R.T."]),
    ("no_disks", ["No se encontraron discos", "No disks found", "Nenhum disco encontrado", "Aucun disque trouvé", "Keine Laufwerke gefunden", "Nessun disco trovato"]),
    ("health_status", ["Estado de salud", "Health status", "Estado de saúde", "État de santé", "Zustand", "Stato di salute"]),
    ("temperature", ["Temperatura", "Temperature", "Temperatura", "Température", "Temperatur", "Temperatura"]),
    ("model", ["Modelo", "Model", "Modelo", "Modèle", "Modell", "Modello"]),
    ("capacity", ["Capacidad", "Disk size", "Capacidade", "Capacité", "Kapazität", "Capacità"]),
    ("firmware", ["Firmware", "Firmware", "Firmware", "Firmware", "Firmware", "Firmware"]),
    ("serial", ["Número de serie", "Serial number", "Número de série", "Numéro de série", "Seriennummer", "Numero di serie"]),
    ("interface", ["Interfaz", "Interface", "Interface", "Interface", "Schnittstelle", "Interfaccia"]),
    ("transfer_mode", ["Modo de transferencia", "Transfer mode", "Modo de transferência", "Mode de transfert", "Übertragungsmodus", "Modalità di trasferimento"]),
    ("mounts", ["Montajes", "Mount points", "Montagens", "Points de montage", "Einhängepunkte", "Punti di montaggio"]),
    ("host_reads", ["Lecturas del host", "Host reads", "Leituras do host", "Lectures hôte", "Host-Lesevorgänge", "Letture host"]),
    ("host_writes", ["Escrituras del host", "Host writes", "Gravações do host", "Écritures hôte", "Host-Schreibvorgänge", "Scritture host"]),
    ("rotation", ["Velocidad de rotación", "Rotation rate", "Velocidade de rotação", "Vitesse de rotation", "Drehzahl", "Velocità di rotazione"]),
    ("power_on_count", ["Ciclos de encendido", "Power on count", "Ciclos de ligação", "Nombre de démarrages", "Einschaltvorgänge", "Accensioni"]),
    ("power_on_hours", ["Horas encendido", "Power on hours", "Horas ligado", "Heures de fonctionnement", "Betriebsstunden", "Ore di accensione"]),
    ("standard", ["Estándar", "Standard", "Padrão", "Norme", "Standard", "Standard"]),
    ("features", ["Características", "Features", "Recursos", "Fonctions", "Funktionen", "Funzionalità"]),
    ("col_name", ["Nombre del atributo", "Attribute name", "Nome do atributo", "Nom de l'attribut", "Attributname", "Nome attributo"]),
    ("col_current", ["Actual", "Current", "Atual", "Actuelle", "Aktuell", "Attuale"]),
    ("col_worst", ["Peor", "Worst", "Pior", "Pire", "Schlechtester", "Peggiore"]),
    ("col_threshold", ["Umbral", "Threshold", "Limite", "Seuil", "Grenzwert", "Soglia"]),
    ("col_raw", ["Valores en bruto", "Raw values", "Valores brutos", "Valeurs brutes", "Rohwerte", "Valori grezzi"]),
    ("no_smart", ["Sin datos S.M.A.R.T. todavía", "No S.M.A.R.T. data yet", "Ainda sem dados S.M.A.R.T.", "Pas encore de données S.M.A.R.T.", "Noch keine S.M.A.R.T.-Daten", "Nessun dato S.M.A.R.T. per ora"]),
    ("thresholds_title", ["Umbrales de aviso", "Warning thresholds", "Limites de aviso", "Seuils d'alerte", "Warnschwellen", "Soglie di avviso"]),
    ("th_05", ["05 Sectores reasignados", "05 Reallocated sectors", "05 Setores realocados", "05 Secteurs réalloués", "05 Neu zugewiesene Sektoren", "05 Settori riallocati"]),
    ("th_c5", ["C5 Sectores pendientes", "C5 Pending sectors", "C5 Setores pendentes", "C5 Secteurs en attente", "C5 Schwebende Sektoren", "C5 Settori in sospeso"]),
    ("th_c6", ["C6 Sectores no corregibles", "C6 Uncorrectable sectors", "C6 Setores incorrigíveis", "C6 Secteurs non corrigibles", "C6 Nicht korrigierbare Sektoren", "C6 Settori non correggibili"]),
    ("th_ff", ["Vida útil restante del SSD (%)", "SSD remaining life (%)", "Vida útil restante do SSD (%)", "Durée de vie restante du SSD (%)", "Verbleibende SSD-Lebensdauer (%)", "Vita residua SSD (%)"]),
    ("th_temp", ["Alarma de temperatura (°C)", "Temperature alarm (°C)", "Alarme de temperatura (°C)", "Alarme de température (°C)", "Temperaturalarm (°C)", "Allarme temperatura (°C)"]),
    ("th_zero_hint", ["0 desactiva el aviso de ese atributo.", "0 turns off the warning for that attribute.", "0 desativa o aviso desse atributo.", "0 désactive l'alerte pour cet attribut.", "0 schaltet die Warnung für dieses Attribut aus.", "0 disattiva l'avviso per quell'attributo."]),
    ("default", ["Por defecto", "Default", "Padrão", "Par défaut", "Standard", "Predefinito"]),
    ("apply", ["Aplicar", "Apply", "Aplicar", "Appliquer", "Übernehmen", "Applica"]),
    ("about_1", ["Muestra la salud y la temperatura de tus discos en Linux, Windows y macOS.", "Shows the health and temperature of your disks on Linux, Windows and macOS.", "Mostra a saúde e a temperatura dos seus discos no Linux, Windows e macOS.", "Affiche la santé et la température de vos disques sous Linux, Windows et macOS.", "Zeigt Zustand und Temperatur Ihrer Laufwerke unter Linux, Windows und macOS.", "Mostra la salute e la temperatura dei tuoi dischi su Linux, Windows e macOS."]),
    ("about_2", ["Los datos S.M.A.R.T. se leen con smartctl (smartmontools).", "S.M.A.R.T. data is read with smartctl (smartmontools).", "Os dados S.M.A.R.T. são lidos com o smartctl (smartmontools).", "Les données S.M.A.R.T. sont lues avec smartctl (smartmontools).", "S.M.A.R.T.-Daten werden mit smartctl (smartmontools) gelesen.", "I dati S.M.A.R.T. vengono letti con smartctl (smartmontools)."]),
    ("about_credit", [
        "Basado en CrystalDiskInfo de hiyohiyo (licencia MIT).",
        "Based on CrystalDiskInfo by hiyohiyo (MIT license).",
        "Baseado no CrystalDiskInfo de hiyohiyo (licença MIT).",
        "Basé sur CrystalDiskInfo de hiyohiyo (licence MIT).",
        "Auf Basis von CrystalDiskInfo von hiyohiyo (MIT-Lizenz).",
        "Basato su CrystalDiskInfo di hiyohiyo (licenza MIT).",
    ]),
    ("admin_banner_win", [
        "Para leer la salud (S.M.A.R.T.) de los discos, DiskInfo debe abrirse como administrador.",
        "To read disk health (S.M.A.R.T.), DiskInfo must run as administrator.",
        "Para ler a saúde (S.M.A.R.T.) dos discos, o DiskInfo deve ser aberto como administrador.",
        "Pour lire la santé (S.M.A.R.T.) des disques, DiskInfo doit être lancé en administrateur.",
        "Zum Lesen des Laufwerkszustands (S.M.A.R.T.) muss DiskInfo als Administrator laufen.",
        "Per leggere la salute (S.M.A.R.T.) dei dischi, DiskInfo deve essere avviato come amministratore.",
    ]),
    ("restart_admin", ["Reiniciar como administrador", "Restart as administrator", "Reiniciar como administrador", "Relancer en administrateur", "Als Administrator neu starten", "Riavvia come amministratore"]),
    ("pw_menu", ["Lectura sin contraseña…", "Read without password…", "Leitura sem senha…", "Lecture sans mot de passe…", "Lesen ohne Passwort…", "Lettura senza password…"]),
    ("pw_banner_btn", ["No volver a pedir contraseña…", "Stop asking for the password…", "Não pedir mais a senha…", "Ne plus demander le mot de passe…", "Nicht mehr nach Passwort fragen…", "Non chiedere più la password…"]),
    ("pw_explain", [
        "Puedes elegir cómo se leen los datos S.M.A.R.T.:\n• Con contraseña (como ahora): se pide la contraseña de administrador cada vez que lees los discos.\n• Sin contraseña: se instala una regla del sistema que permite solo a tu usuario, sentado delante del equipo, leer la salud de los discos sin contraseña. No permite nada más (ni otros programas ni cambiar ajustes de los discos). Activarla o desactivarla pide la contraseña una vez.",
        "You can choose how S.M.A.R.T. data is read:\n• With password (as now): the administrator password is asked each time the disks are read.\n• Without password: a system rule is installed that lets only your user, sitting at this computer, read disk health without a password. It allows nothing else (no other programs, no changing disk settings). Turning it on or off asks for the password once.",
        "Você pode escolher como os dados S.M.A.R.T. são lidos:\n• Com senha (como agora): a senha de administrador é pedida cada vez que os discos são lidos.\n• Sem senha: é instalada uma regra do sistema que permite só ao seu usuário, sentado diante do computador, ler a saúde dos discos sem senha. Não permite mais nada (nem outros programas nem mudar ajustes dos discos). Ativar ou desativar pede a senha uma vez.",
        "Vous pouvez choisir comment les données S.M.A.R.T. sont lues :\n• Avec mot de passe (comme maintenant) : le mot de passe administrateur est demandé à chaque lecture.\n• Sans mot de passe : une règle système est installée qui permet seulement à votre utilisateur, devant cet ordinateur, de lire la santé des disques sans mot de passe. Elle ne permet rien d'autre (ni autres programmes, ni modification des réglages des disques). L'activer ou la désactiver demande le mot de passe une fois.",
        "Sie können wählen, wie S.M.A.R.T.-Daten gelesen werden:\n• Mit Passwort (wie bisher): Das Administratorpasswort wird bei jedem Lesen abgefragt.\n• Ohne Passwort: Eine Systemregel wird installiert, die nur Ihrem Benutzer am Rechner erlaubt, den Laufwerkszustand ohne Passwort zu lesen. Sonst erlaubt sie nichts (keine anderen Programme, keine Änderung von Laufwerkseinstellungen). Ein- oder Ausschalten fragt einmal nach dem Passwort.",
        "Puoi scegliere come leggere i dati S.M.A.R.T.:\n• Con password (come ora): la password di amministratore viene chiesta a ogni lettura dei dischi.\n• Senza password: viene installata una regola di sistema che permette solo al tuo utente, seduto davanti al computer, di leggere la salute dei dischi senza password. Non permette nient'altro (né altri programmi né modifiche alle impostazioni dei dischi). Attivarla o disattivarla chiede la password una volta.",
    ]),
    ("pw_state_on", ["Estado: sin contraseña (activada)", "Status: without password (on)", "Estado: sem senha (ativada)", "État : sans mot de passe (activée)", "Status: ohne Passwort (aktiv)", "Stato: senza password (attiva)"]),
    ("pw_state_off", ["Estado: con contraseña", "Status: with password", "Estado: com senha", "État : avec mot de passe", "Status: mit Passwort", "Stato: con password"]),
    ("pw_enable", ["Activar lectura sin contraseña", "Turn on reading without password", "Ativar leitura sem senha", "Activer la lecture sans mot de passe", "Lesen ohne Passwort einschalten", "Attiva lettura senza password"]),
    ("pw_disable", ["Volver a pedir contraseña", "Ask for the password again", "Voltar a pedir a senha", "Redemander le mot de passe", "Wieder nach Passwort fragen", "Richiedi di nuovo la password"]),
    ("pw_working", ["Esperando la contraseña…", "Waiting for the password…", "Aguardando a senha…", "En attente du mot de passe…", "Warte auf Passwort…", "In attesa della password…"]),
    ("msg_rule_on", ["Lectura sin contraseña activada.", "Reading without password is on.", "Leitura sem senha ativada.", "Lecture sans mot de passe activée.", "Lesen ohne Passwort ist aktiv.", "Lettura senza password attivata."]),
    ("msg_rule_off", ["Se volverá a pedir la contraseña.", "The password will be asked again.", "A senha voltará a ser pedida.", "Le mot de passe sera de nouveau demandé.", "Das Passwort wird wieder abgefragt.", "La password verrà chiesta di nuovo."]),
    ("msg_rule_failed", ["No se pudo cambiar la regla de permisos.", "Could not change the permission rule.", "Não foi possível alterar a regra de permissões.", "Impossible de modifier la règle d'autorisation.", "Die Berechtigungsregel konnte nicht geändert werden.", "Impossibile modificare la regola dei permessi."]),
    // Help shown when the pointer rests on a field.
    ("h_health", [
        "Resumen de la salud del disco a partir de sus datos S.M.A.R.T.: Bueno, Precaución (vigílalo y haz copia de seguridad), Malo (cámbialo pronto) o Desconocido (sin datos). En SSD se muestra también la vida útil restante.",
        "Summary of the disk's health from its S.M.A.R.T. data: Good, Caution (watch it and back up), Bad (replace it soon) or Unknown (no data). SSDs also show their remaining life.",
        "Resumo da saúde do disco a partir dos dados S.M.A.R.T.: Bom, Atenção (vigie e faça backup), Ruim (troque logo) ou Desconhecido (sem dados). Nos SSD mostra também a vida útil restante.",
        "Résumé de la santé du disque d'après ses données S.M.A.R.T. : Bon, Prudence (surveillez-le et sauvegardez), Mauvais (à remplacer bientôt) ou Inconnu (pas de données). Les SSD affichent aussi leur durée de vie restante.",
        "Zusammenfassung des Laufwerkszustands aus den S.M.A.R.T.-Daten: Gut, Vorsicht (beobachten und sichern), Schlecht (bald ersetzen) oder Unbekannt (keine Daten). Bei SSDs auch die verbleibende Lebensdauer.",
        "Riepilogo della salute del disco dai dati S.M.A.R.T.: Buono, Attenzione (tienilo d'occhio e fai un backup), Cattivo (sostituiscilo presto) o Sconosciuto (nessun dato). Negli SSD mostra anche la vita residua.",
    ]),
    ("h_temperature", [
        "Temperatura actual del disco. Se pone en rojo al llegar a la alarma (50 °C en discos duros, 60 °C en SSD; se cambia en Umbrales de aviso).",
        "Current disk temperature. It turns red at the alarm level (50 °C for hard disks, 60 °C for SSDs; change it in Warning thresholds).",
        "Temperatura atual do disco. Fica vermelha ao atingir o alarme (50 °C em HDs, 60 °C em SSD; altere em Limites de aviso).",
        "Température actuelle du disque. Elle passe au rouge au seuil d'alarme (50 °C pour les disques durs, 60 °C pour les SSD ; modifiable dans Seuils d'alerte).",
        "Aktuelle Laufwerkstemperatur. Wird rot beim Alarmwert (50 °C bei Festplatten, 60 °C bei SSDs; änderbar unter Warnschwellen).",
        "Temperatura attuale del disco. Diventa rossa alla soglia di allarme (50 °C per HDD, 60 °C per SSD; modificabile in Soglie di avviso).",
    ]),
    ("h_firmware", ["Versión del programa interno del disco. Útil para buscar actualizaciones del fabricante.", "Version of the disk's internal software. Useful to look for manufacturer updates.", "Versão do software interno do disco. Útil para procurar atualizações do fabricante.", "Version du logiciel interne du disque. Utile pour chercher des mises à jour du fabricant.", "Version der internen Laufwerkssoftware. Nützlich für Herstellerupdates.", "Versione del software interno del disco. Utile per cercare aggiornamenti del produttore."]),
    ("h_serial", ["Número único que identifica este disco (para garantías o soporte).", "Unique number that identifies this disk (for warranty or support).", "Número único que identifica este disco (para garantia ou suporte).", "Numéro unique qui identifie ce disque (garantie ou support).", "Eindeutige Nummer dieses Laufwerks (für Garantie oder Support).", "Numero unico che identifica il disco (per garanzia o assistenza)."]),
    ("h_interface", ["Cómo se conecta el disco al equipo: Serial ATA (SATA), NVM Express (NVMe, en ranura M.2/PCIe) o USB.", "How the disk connects to the computer: Serial ATA (SATA), NVM Express (NVMe, M.2/PCIe slot) or USB.", "Como o disco se conecta: Serial ATA (SATA), NVM Express (NVMe, slot M.2/PCIe) ou USB.", "Connexion du disque : Serial ATA (SATA), NVM Express (NVMe, emplacement M.2/PCIe) ou USB.", "Anschluss des Laufwerks: Serial ATA (SATA), NVM Express (NVMe, M.2/PCIe) oder USB.", "Come si collega il disco: Serial ATA (SATA), NVM Express (NVMe, slot M.2/PCIe) o USB."]),
    ("h_transfer_mode", ["Velocidad de conexión: la actual | la máxima que admite el disco. Si la actual es menor, puede ir conectado a un puerto más lento.", "Link speed: current | the most the disk supports. If current is lower, it may be on a slower port.", "Velocidade da conexão: atual | máxima suportada. Se a atual for menor, pode estar numa porta mais lenta.", "Vitesse de liaison : actuelle | maximale prise en charge. Si l'actuelle est plus basse, le port est peut-être plus lent.", "Verbindungsgeschwindigkeit: aktuell | maximal unterstützt. Ist sie niedriger, steckt es evtl. an einem langsameren Anschluss.", "Velocità di collegamento: attuale | massima supportata. Se l'attuale è più bassa, potrebbe essere su una porta più lenta."]),
    ("h_mounts", ["Carpetas donde están montadas las particiones de este disco (equivale a las letras de unidad de Windows).", "Folders where this disk's partitions are mounted (like drive letters on Windows).", "Pastas onde as partições deste disco estão montadas (como as letras de unidade do Windows).", "Dossiers où sont montées les partitions de ce disque (comme les lettres de lecteur sous Windows).", "Ordner, in denen die Partitionen eingehängt sind (wie Laufwerksbuchstaben unter Windows).", "Cartelle in cui sono montate le partizioni del disco (come le lettere di unità di Windows)."]),
    ("h_host_reads", ["Datos totales que el equipo ha leído del disco en toda su vida.", "Total data the computer has read from the disk over its life.", "Total de dados que o computador leu do disco em toda a vida.", "Total des données lues sur le disque pendant sa vie.", "Insgesamt vom Laufwerk gelesene Daten seit Inbetriebnahme.", "Dati totali letti dal disco in tutta la sua vita."]),
    ("h_host_writes", ["Datos totales escritos en el disco. En los SSD el desgaste depende sobre todo de esta cifra.", "Total data written to the disk. On SSDs wear depends mostly on this figure.", "Total de dados gravados no disco. Nos SSD o desgaste depende sobretudo deste valor.", "Total des données écrites. Sur un SSD, l'usure dépend surtout de ce chiffre.", "Insgesamt geschriebene Daten. Bei SSDs hängt der Verschleiß vor allem davon ab.", "Dati totali scritti sul disco. Negli SSD l'usura dipende soprattutto da questo valore."]),
    ("h_rotation", ["Vueltas por minuto de los platos de un disco duro. Los SSD no giran.", "Revolutions per minute of a hard disk's platters. SSDs don't spin.", "Rotações por minuto dos pratos de um HD. SSD não giram.", "Tours par minute des plateaux d'un disque dur. Les SSD ne tournent pas.", "Umdrehungen pro Minute der Festplattenscheiben. SSDs drehen sich nicht.", "Giri al minuto dei piatti di un HDD. Gli SSD non ruotano."]),
    ("h_power_on_count", ["Veces que se ha encendido el disco.", "How many times the disk has been powered on.", "Quantas vezes o disco foi ligado.", "Nombre de mises sous tension du disque.", "Wie oft das Laufwerk eingeschaltet wurde.", "Quante volte il disco è stato acceso."]),
    ("h_power_on_hours", ["Horas totales que el disco ha estado encendido.", "Total hours the disk has been powered on.", "Total de horas que o disco ficou ligado.", "Nombre total d'heures de fonctionnement.", "Gesamte Betriebsstunden des Laufwerks.", "Ore totali di accensione del disco."]),
    ("h_standard", ["Versión de la norma (ATA/SATA o NVMe) que cumple el disco.", "Version of the standard (ATA/SATA or NVMe) the disk follows.", "Versão da norma (ATA/SATA ou NVMe) que o disco segue.", "Version de la norme (ATA/SATA ou NVMe) suivie par le disque.", "Version des Standards (ATA/SATA oder NVMe) des Laufwerks.", "Versione dello standard (ATA/SATA o NVMe) del disco."]),
    ("h_features", ["Funciones que admite el disco: S.M.A.R.T. (autodiagnóstico), TRIM (mantiene rápidos los SSD), 48bit LBA (discos grandes), APM (ahorro de energía), VolatileWriteCache (caché de escritura).", "Functions the disk supports: S.M.A.R.T. (self-monitoring), TRIM (keeps SSDs fast), 48bit LBA (large disks), APM (power saving), VolatileWriteCache (write cache).", "Funções suportadas: S.M.A.R.T. (autodiagnóstico), TRIM (mantém o SSD rápido), 48bit LBA (discos grandes), APM (economia de energia), VolatileWriteCache (cache de gravação).", "Fonctions prises en charge : S.M.A.R.T. (autosurveillance), TRIM (garde les SSD rapides), 48bit LBA (grands disques), APM (économie d'énergie), VolatileWriteCache (cache d'écriture).", "Unterstützte Funktionen: S.M.A.R.T. (Selbstüberwachung), TRIM (hält SSDs schnell), 48bit LBA (große Laufwerke), APM (Energiesparen), VolatileWriteCache (Schreibcache).", "Funzioni supportate: S.M.A.R.T. (autodiagnosi), TRIM (mantiene veloci gli SSD), 48bit LBA (dischi grandi), APM (risparmio energetico), VolatileWriteCache (cache di scrittura)."]),
    ("h_col_current", ["Valor normalizado actual (normalmente 100 o 200 es lo mejor; baja con el desgaste).", "Current normalized value (usually 100 or 200 is best; it drops with wear).", "Valor normalizado atual (normalmente 100 ou 200 é o melhor; cai com o desgaste).", "Valeur normalisée actuelle (100 ou 200 est généralement le mieux ; baisse avec l'usure).", "Aktueller normierter Wert (meist ist 100 oder 200 am besten; sinkt mit Verschleiß).", "Valore normalizzato attuale (di solito 100 o 200 è il migliore; cala con l'usura)."]),
    ("h_col_worst", ["El valor más bajo que ha tenido nunca.", "The lowest value it has ever had.", "O menor valor que já teve.", "La plus basse valeur jamais atteinte.", "Der niedrigste jemals erreichte Wert.", "Il valore più basso mai raggiunto."]),
    ("h_col_threshold", ["Si el valor actual baja de este umbral, el fabricante considera que el atributo ha fallado (estado Malo).", "If the current value falls below this threshold, the maker considers the attribute failed (Bad).", "Se o valor atual cair abaixo deste limite, o fabricante considera o atributo falho (Ruim).", "Si la valeur actuelle passe sous ce seuil, le fabricant considère l'attribut en échec (Mauvais).", "Fällt der aktuelle Wert unter diesen Grenzwert, gilt das Attribut als fehlerhaft (Schlecht).", "Se il valore attuale scende sotto questa soglia, il produttore considera l'attributo guasto (Cattivo)."]),
    ("h_col_raw", ["Contador real del disco en hexadecimal (pasa el ratón por una fila para verlo en decimal).", "The disk's real counter in hexadecimal (hover a row to see it in decimal).", "Contador real do disco em hexadecimal (passe o mouse numa linha para ver em decimal).", "Compteur réel du disque en hexadécimal (survolez une ligne pour le voir en décimal).", "Echter Zähler des Laufwerks hexadezimal (Zeile überfahren für Dezimalwert).", "Contatore reale del disco in esadecimale (passa sopra una riga per vederlo in decimale)."]),
    ("h_tab", ["Haz clic para ver este disco. El color indica su estado de salud.", "Click to show this disk. The colour shows its health.", "Clique para ver este disco. A cor indica a saúde.", "Cliquez pour afficher ce disque. La couleur indique sa santé.", "Klicken, um dieses Laufwerk zu zeigen. Die Farbe zeigt den Zustand.", "Fai clic per vedere questo disco. Il colore indica la salute."]),
    ("raw_decimal", ["Valor en bruto (decimal)", "Raw value (decimal)", "Valor bruto (decimal)", "Valeur brute (décimale)", "Rohwert (dezimal)", "Valore grezzo (decimale)"]),
    ("menu_entry", ["Mostrar en el menú de aplicaciones", "Show in the application menu", "Mostrar no menu de aplicativos", "Afficher dans le menu des applications", "Im Anwendungsmenü anzeigen", "Mostra nel menu delle applicazioni"]),
    ("unit_hours", ["horas", "hours", "horas", "heures", "Stunden", "ore"]),
    ("unit_times", ["veces", "count", "vezes", "fois", "mal", "volte"]),
    ("good", ["Bueno", "Good", "Bom", "Bon", "Gut", "Buono"]),
    ("caution", ["Precaución", "Caution", "Atenção", "Prudence", "Vorsicht", "Attenzione"]),
    ("bad", ["Malo", "Bad", "Ruim", "Mauvais", "Schlecht", "Cattivo"]),
    ("unknown", ["Desconocido", "Unknown", "Desconhecido", "Inconnu", "Unbekannt", "Sconosciuto"]),
    ("report_title", ["Información de discos (Linux)", "Disk information (Linux)", "Informações dos discos (Linux)", "Informations sur les disques (Linux)", "Laufwerksinformationen (Linux)", "Informazioni sui dischi (Linux)"]),
    ("msg_no_smartctl", [
        "No se encontró «smartctl». Instala el paquete smartmontools para ver la salud de los discos.",
        "“smartctl” was not found. Install the smartmontools package to see disk health.",
        "“smartctl” não foi encontrado. Instale o pacote smartmontools para ver a saúde dos discos.",
        "« smartctl » est introuvable. Installez le paquet smartmontools pour voir la santé des disques.",
        "„smartctl“ wurde nicht gefunden. Installieren Sie smartmontools, um den Laufwerkszustand zu sehen.",
        "«smartctl» non trovato. Installa il pacchetto smartmontools per vedere la salute dei dischi.",
    ]),
    ("msg_no_pkexec", [
        "No se pudo pedir permisos de administrador (falta pkexec).",
        "Could not ask for administrator rights (pkexec is missing).",
        "Não foi possível pedir permissões de administrador (falta o pkexec).",
        "Impossible de demander les droits d'administrateur (pkexec manquant).",
        "Administratorrechte konnten nicht angefordert werden (pkexec fehlt).",
        "Impossibile richiedere i permessi di amministratore (manca pkexec).",
    ]),
    ("msg_denied", [
        "No se concedieron permisos de administrador.",
        "Administrator rights were not granted.",
        "As permissões de administrador não foram concedidas.",
        "Les droits d'administrateur n'ont pas été accordés.",
        "Administratorrechte wurden nicht gewährt.",
        "I permessi di amministratore non sono stati concessi.",
    ]),
];

/// The text for `key` in `lang`; the key itself when missing.
pub fn t(lang: Lang, key: &'static str) -> &'static str {
    TEXTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v[lang.index()])
        .unwrap_or(key)
}

/// Attribute names in Spanish and English (other languages use English).
const ATA_NAMES: &[(u16, &str, &str)] = &[
    (0x01, "Tasa de errores de lectura", "Read error rate"),
    (0x02, "Rendimiento de transferencia", "Throughput performance"),
    (0x03, "Tiempo de arranque del motor", "Spin-up time"),
    (0x04, "Arranques/paradas del motor", "Start/stop count"),
    (0x05, "Sectores reasignados", "Reallocated sectors count"),
    (0x07, "Tasa de errores de búsqueda", "Seek error rate"),
    (0x08, "Rendimiento de búsqueda", "Seek time performance"),
    (0x09, "Horas encendido", "Power-on hours"),
    (0x0A, "Reintentos de arranque del motor", "Spin retry count"),
    (0x0B, "Reintentos de calibración", "Recalibration retries"),
    (0x0C, "Ciclos de encendido", "Power cycle count"),
    (0xAA, "Bloques de reserva disponibles", "Available reserved space"),
    (0xAB, "Fallos de programación", "Program fail count"),
    (0xAC, "Fallos de borrado", "Erase fail count"),
    (0xAD, "Desgaste medio", "Average erase count"),
    (0xAE, "Apagados inesperados", "Unexpected power loss count"),
    (0xB1, "Nivelación de desgaste", "Wear leveling count"),
    (0xB4, "Bloques de reserva sin usar", "Unused reserved block count"),
    (0xB7, "Errores SATA de bajada de velocidad", "SATA downshift error count"),
    (0xB8, "Errores de extremo a extremo", "End-to-end error"),
    (0xBB, "Errores no corregibles notificados", "Reported uncorrectable errors"),
    (0xBC, "Tiempos de espera de comandos", "Command timeout"),
    (0xBD, "Escrituras a gran altura", "High fly writes"),
    (0xBE, "Temperatura del flujo de aire", "Airflow temperature"),
    (0xBF, "Tasa de errores por golpes", "G-sense error rate"),
    (0xC0, "Retracciones por apagado", "Power-off retract count"),
    (0xC1, "Ciclos de carga/descarga", "Load/unload cycle count"),
    (0xC2, "Temperatura", "Temperature"),
    (0xC3, "Recuperaciones por ECC de hardware", "Hardware ECC recovered"),
    (0xC4, "Eventos de reasignación", "Reallocation event count"),
    (0xC5, "Sectores pendientes", "Current pending sector count"),
    (0xC6, "Sectores no corregibles", "Uncorrectable sector count"),
    (0xC7, "Errores CRC UltraDMA", "UltraDMA CRC error count"),
    (0xC8, "Tasa de errores de escritura", "Write error rate"),
    (0xCA, "Vida útil restante", "Percent lifetime remaining"),
    (0xE7, "Vida útil restante del SSD", "SSD life left"),
    (0xE8, "Reserva disponible", "Available reserved space"),
    (0xE9, "Indicador de desgaste", "Media wearout indicator"),
    (0xF0, "Horas de vuelo del cabezal", "Head flying hours"),
    (0xF1, "Escrituras totales del host", "Total host writes"),
    (0xF2, "Lecturas totales del host", "Total host reads"),
];

const NVME_NAMES: &[(u16, &str, &str)] = &[
    (0x01, "Advertencia crítica", "Critical warning"),
    (0x02, "Temperatura compuesta", "Composite temperature"),
    (0x03, "Reserva disponible", "Available spare"),
    (0x04, "Umbral de reserva disponible", "Available spare threshold"),
    (0x05, "Porcentaje usado", "Percentage used"),
    (0x06, "Unidades de datos leídas", "Data units read"),
    (0x07, "Unidades de datos escritas", "Data units written"),
    (0x08, "Comandos de lectura del host", "Host read commands"),
    (0x09, "Comandos de escritura del host", "Host write commands"),
    (0x0A, "Tiempo ocupado del controlador", "Controller busy time"),
    (0x0B, "Ciclos de encendido", "Power cycles"),
    (0x0C, "Horas encendido", "Power on hours"),
    (0x0D, "Apagados inseguros", "Unsafe shutdowns"),
    (0x0E, "Errores de integridad de datos", "Media and data integrity errors"),
    (0x0F, "Entradas del registro de errores", "Error information log entries"),
];

/// Short explanations of the attributes that matter most (ATA id, NVMe flag).
const ATTR_HELP: &[(u16, bool, [&str; 6])] = &[
    (0x05, false, ["Sectores dañados que el disco ha sustituido por otros de reserva. Si sube, el disco se está deteriorando.", "Damaged sectors the disk replaced with spare ones. If it grows, the disk is deteriorating.", "Setores danificados substituídos por reservas. Se aumentar, o disco está se deteriorando.", "Secteurs endommagés remplacés par des secteurs de réserve. S'il augmente, le disque se dégrade.", "Beschädigte Sektoren, die durch Reserve ersetzt wurden. Steigt der Wert, verschleißt das Laufwerk.", "Settori danneggiati sostituiti con quelli di riserva. Se aumenta, il disco si sta deteriorando."]),
    (0xC5, false, ["Sectores inestables esperando a ser reasignados. Cualquier valor mayor que 0 es motivo de precaución.", "Unstable sectors waiting to be reallocated. Anything above 0 is a reason for caution.", "Setores instáveis aguardando realocação. Qualquer valor acima de 0 pede atenção.", "Secteurs instables en attente de réallocation. Toute valeur au-dessus de 0 appelle à la prudence.", "Instabile Sektoren, die auf Neuzuweisung warten. Jeder Wert über 0 ist ein Warnzeichen.", "Settori instabili in attesa di riallocazione. Qualsiasi valore sopra 0 richiede attenzione."]),
    (0xC6, false, ["Sectores que no se pudieron leer ni corregir. Puede haber datos perdidos: haz copia de seguridad.", "Sectors that could not be read or corrected. Data may be lost: back up.", "Setores que não puderam ser lidos nem corrigidos. Pode haver perda de dados: faça backup.", "Secteurs illisibles et non corrigibles. Des données peuvent être perdues : sauvegardez.", "Sektoren, die weder gelesen noch korrigiert werden konnten. Datenverlust möglich: sichern!", "Settori non leggibili né correggibili. Possibile perdita di dati: fai un backup."]),
    (0xC7, false, ["Errores en el cable o la conexión. Si sube, revisa o cambia el cable SATA.", "Errors in the cable or connection. If it grows, check or replace the SATA cable.", "Erros no cabo ou conexão. Se aumentar, verifique ou troque o cabo SATA.", "Erreurs de câble ou de connexion. S'il augmente, vérifiez ou changez le câble SATA.", "Fehler in Kabel oder Verbindung. Steigt er, SATA-Kabel prüfen oder tauschen.", "Errori del cavo o della connessione. Se aumenta, controlla o sostituisci il cavo SATA."]),
    (0xBB, false, ["Errores que el disco no pudo corregir al leer. Deberían ser 0.", "Errors the disk could not correct when reading. Should be 0.", "Erros que o disco não conseguiu corrigir na leitura. Deveriam ser 0.", "Erreurs non corrigées à la lecture. Devrait être 0.", "Beim Lesen nicht korrigierbare Fehler. Sollte 0 sein.", "Errori non corretti in lettura. Dovrebbero essere 0."]),
    (0x09, false, ["Horas totales encendido.", "Total powered-on hours.", "Total de horas ligado.", "Heures totales de fonctionnement.", "Gesamte Betriebsstunden.", "Ore totali di accensione."]),
    (0x0C, false, ["Número de veces que se ha encendido.", "Number of times it was powered on.", "Número de vezes que foi ligado.", "Nombre de mises sous tension.", "Anzahl der Einschaltvorgänge.", "Numero di accensioni."]),
    (0xC2, false, ["Temperatura del disco (los últimos dígitos en bruto suelen ser los grados).", "Disk temperature (the last raw digits are usually the degrees).", "Temperatura do disco (os últimos dígitos brutos costumam ser os graus).", "Température du disque (les derniers chiffres bruts sont souvent les degrés).", "Laufwerkstemperatur (die letzten Rohziffern sind meist die Grad).", "Temperatura del disco (le ultime cifre grezze sono di solito i gradi)."]),
    (0x01, true, ["Distinto de 0 indica un problema grave (poca reserva, temperatura, solo lectura…).", "Non-zero means a serious problem (low spare, temperature, read-only…).", "Diferente de 0 indica um problema grave (pouca reserva, temperatura, somente leitura…).", "Différent de 0 : problème grave (réserve faible, température, lecture seule…).", "Ungleich 0 bedeutet ein ernstes Problem (wenig Reserve, Temperatur, schreibgeschützt…).", "Diverso da 0 indica un problema grave (riserva bassa, temperatura, sola lettura…)."]),
    (0x03, true, ["Porcentaje de espacio de reserva que queda para sustituir celdas gastadas.", "Percentage of spare space left to replace worn cells.", "Percentagem de espaço de reserva restante para substituir células gastas.", "Pourcentage de réserve restant pour remplacer les cellules usées.", "Verbleibende Reserve in Prozent zum Ersetzen verschlissener Zellen.", "Percentuale di spazio di riserva rimasto per sostituire le celle usurate."]),
    (0x04, true, ["Si la reserva disponible baja de este valor, el disco se considera en mal estado.", "If available spare falls below this, the disk is considered bad.", "Se a reserva cair abaixo deste valor, o disco é considerado ruim.", "Si la réserve passe sous cette valeur, le disque est considéré comme mauvais.", "Fällt die Reserve darunter, gilt das Laufwerk als schlecht.", "Se la riserva scende sotto questo valore, il disco è considerato guasto."]),
    (0x05, true, ["Desgaste estimado por el fabricante. 100 % significa que llegó a su vida prevista (vida restante = 100 − este valor).", "Wear estimated by the maker. 100 % means it reached its expected life (remaining life = 100 − this value).", "Desgaste estimado pelo fabricante. 100 % significa vida prevista atingida (vida restante = 100 − este valor).", "Usure estimée par le fabricant. 100 % = durée de vie prévue atteinte (vie restante = 100 − cette valeur).", "Vom Hersteller geschätzter Verschleiß. 100 % = erwartete Lebensdauer erreicht (Rest = 100 − Wert).", "Usura stimata dal produttore. 100 % = vita prevista raggiunta (vita residua = 100 − questo valore)."]),
    (0x0E, true, ["Errores de datos que el controlador no pudo corregir. Deberían ser 0.", "Data errors the controller could not correct. Should be 0.", "Erros de dados que o controlador não corrigiu. Deveriam ser 0.", "Erreurs de données non corrigées par le contrôleur. Devrait être 0.", "Vom Controller nicht korrigierte Datenfehler. Sollte 0 sein.", "Errori di dati non corretti dal controller. Dovrebbero essere 0."]),
    (0x0D, true, ["Veces que el disco se apagó de golpe (corte de luz, botón de apagado).", "Times the disk lost power abruptly (power cut, power button).", "Vezes que o disco desligou de repente (queda de energia, botão).", "Coupures brutales d'alimentation (panne, bouton).", "Wie oft der Strom abrupt weg war (Stromausfall, Ausschalter).", "Spegnimenti improvvisi (blackout, pulsante di accensione)."]),
];

pub fn attr_help(lang: Lang, id: u16, nvme: bool) -> Option<&'static str> {
    ATTR_HELP.iter().find(|(i, n, _)| *i == id && *n == nvme).map(|(_, _, v)| v[lang.index()])
}

/// Display name for an attribute; `fallback` is the drive's own name.
pub fn attr_name(lang: Lang, id: u16, nvme: bool, fallback: &str) -> String {
    let table = if nvme { NVME_NAMES } else { ATA_NAMES };
    match table.iter().find(|(i, _, _)| *i == id) {
        Some((_, es, _)) if lang == Lang::Es => es.to_string(),
        Some((_, _, en)) => en.to_string(),
        None => fallback.replace('_', " "),
    }
}
