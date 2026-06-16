# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<h3 align="center">KI-gestützter Programmierassistent — Desktop-Anwendung</h3>

<p align="center">
  Mehr als nur Chat — eine echte KI-Werkbank, die Code schreibt, Befehle ausführt und Aufgaben für Sie erledigt
</p>

<p align="center">
  <a href="README.md">中文</a> |
  <a href="README.en.md">English</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="README.fr.md">Français</a> |
  <a href="README.de.md">Deutsch</a>
</p>

<p align="center">
  <a href="http://47.113.221.244:8081/">Webseite</a> •
  <a href="http://47.113.221.244:8081/usage.html">Benutzerhandbuch</a> •
  <a href="https://github.com/longdream/cn-codex">GitHub</a>
</p>

---

## Warum CN-Codex?

Herkömmliche KI-Codierungsassistenten können nur chatten. CN-Codex ist eine **vollständige KI-Werkbank** — es liest und schreibt direkt Ihren Code, führt Shell-Befehle aus, steuert Browser, verwaltet parallel arbeitende Sub-Agenten und ermöglicht es Ihnen sogar, den KI-Fortschritt per Smartphone aus der Ferne zu überwachen.

- **40+ integrierte Werkzeuge** — Dateioperationen, Befehlsausführung, Browser-Automatisierung, Sub-Agenten, MCP-Protokoll und mehr
- **12+ LLM-Anbieter** — OpenAI, Anthropic, Google, DeepSeek, Volcengine, Tongyi Qianwen, Zhipu, Moonshot, SiliconFlow, Baichuan, Ollama, LM Studio
- **Ziel-Modus (Goal)** — Setzen Sie ein Ziel und lassen Sie die KI autonom mehrstufige Aufgaben planen und ausführen
- **Plugins + Skills + Roboter** — Ein erweiterbares Automatisierungssystem zum Aufbau eigener KI-Workflows
- **Mobile Synchronisation per QR-Code** — LAN-Direktverbindung oder öffentliches Relay, steuern Sie Ihren KI-Assistenten von überall
- **Sofort einsatzbereit** — Herunterladen, Doppelklick und loslegen. Keine CLI-Installation oder Umgebungsvariablen erforderlich

---

## Oberflächen-Vorschau

<p align="center">
  <img src="docs/screenshot-main.png" alt="Hauptoberfläche" width="800">
</p>
<p align="center"><em>Erster Start — Klares dunkles Interface mit Projektverwaltung links und Chat-Bereich in der Mitte</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-chat.png" alt="Chat-Oberfläche" width="800">
</p>
<p align="center"><em>Intelligenter Chat — Streaming-Ausgabe, Modellwechsel, automatische Genehmigung, Statusleiste auf einen Blick</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-project.png" alt="Projekt-Modus" width="800">
</p>
<p align="center"><em>Projekt-Modus — Chat/Ziel-Dual-Modus-Umschaltung, Roboter-Auswahl, KI arbeitet in Ihrem Projektverzeichnis</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-provider.png" alt="Anbieter-Einstellungen" width="800">
</p>
<p align="center"><em>Anbieter-Konfiguration — Visuelle GUI zur Verwaltung mehrerer LLM-Anbieter mit Modelllisten und Vision-Fähigkeits-Tags</em></p>

---

## Kernfähigkeiten

| Fähigkeit | Beschreibung |
|-----------|--------------|
| Intelligenter Chat | Multi-Turn-Kontext, Streaming-Ausgabe, Markdown-Rendering, Sitzungssuche und -verzweigung |
| Werkzeugaufrufe | Datei-I/O, Shell, Browser-Automatisierung, Sub-Agenten, Speicher, MCP — 40+ Werkzeuge |
| Ziel-Modus | KI-autonome Mehrstufenausführung mit Token-Budget-Kontrolle und Statusüberwachung |
| Plugin-System | Browser, Computer Use, Documents, Presentations, Spreadsheets, Sites, Superpowers |
| Roboter | KI erstellt automatisch professionelle Rollen mit Skills und Workflow-Konfigurationen |
| Mobil | Integrierter Webserver + WebSocket, unterstützt LAN-Direkt und öffentliches Relay |
| Terminal-Panel | Eingebettetes xterm.js-Terminal, Multi-Tab, parallel zur KI arbeiten |
| Hooks | Ereignisgesteuerte Automatisierungs-Hooks über den gesamten Agenten-Lebenszyklus |

> Für die vollständige Dokumentation und Anleitungen, siehe das **[Benutzerhandbuch](http://47.113.221.244:8081/usage.html)**

---

## Schnellstart

### 1. Anwendung starten

Doppelklicken Sie auf `CN-Codex.exe` zum Starten. Beim ersten Start wird ein `codey/`-Laufzeitordner im gleichen Verzeichnis erstellt.

### 2. Anbieter konfigurieren

**Einstellungen** → **Modellanbieter** → **Anbieter hinzufügen** → Voreinstellung wählen (z.B. DeepSeek, OpenAI) → API-Key und Basis-URL eingeben → **Speichern** → **Aktivieren**

### 3. Projekt hinzufügen und loslegen

Klicken Sie auf den **Ordner+**-Button in der Seitenleiste, um ein Code-Verzeichnis hinzuzufügen, wählen Sie ein Modell und beginnen Sie zu chatten. Die KI wird alle Operationen in Ihrem Projektverzeichnis ausführen.

> Für detaillierte Schritte und erweiterte Konfiguration, siehe das **[Benutzerhandbuch](http://47.113.221.244:8081/usage.html)**

---

## Technologie-Stack

| Schicht | Technologie |
|---------|-------------|
| Desktop-Framework | Tauri v2 |
| Backend | Rust (Edition 2024) |
| Frontend | React 18 + TypeScript + Vite 6 |
| Styling | Tailwind CSS 4 |
| Zustandsverwaltung | Zustand |
| Internationalisierung | react-intl |
| Datenbank | SQLite |

---

## Links

| Link | Beschreibung |
|------|--------------|
| [Webseite](http://47.113.221.244:8081/) | Produktvorstellung und Download |
| [Benutzerhandbuch](http://47.113.221.244:8081/usage.html) | Vollständige Dokumentation vom ersten Start bis zu erweiterten Funktionen |
| [GitHub](https://github.com/longdream/cn-codex) | Quellcode und Issue-Tracker |

---

## Lizenz

MIT License

---

<p align="center">
  <strong>CN-Codex</strong> — Machen Sie KI zu Ihrem Programmierpartner
</p>
