# CN-Codex

<p align="center">
  <img src="app-icon.svg" alt="CN-Codex Logo" width="128" height="128">
</p>

<h3 align="center">Assistant de programmation IA — Application de bureau</h3>

<p align="center">
  Plus qu'un simple chat — un véritable atelier IA qui écrit du code, exécute des commandes et accomplit des tâches pour vous
</p>

<p align="center">
  <a href="README.md">中文</a> |
  <a href="README.en.md">English</a> |
  <a href="README.ja.md">日本語</a> |
  <a href="README.fr.md">Français</a> |
  <a href="README.de.md">Deutsch</a>
</p>

<p align="center">
  <a href="http://47.113.221.244:8081/">Site officiel</a> •
  <a href="http://47.113.221.244:8081/usage.html">Guide d'utilisation</a> •
  <a href="https://github.com/longdream/cn-codex">GitHub</a>
</p>

---

## Pourquoi CN-Codex ?

Les assistants de codage IA traditionnels ne font que discuter. CN-Codex est un **atelier IA complet** — il lit et écrit directement votre code, exécute des commandes Shell, contrôle des navigateurs, gère des sous-agents travaillant en parallèle, et vous permet même de surveiller la progression de l'IA à distance depuis votre téléphone.

- **40+ outils intégrés** — Opérations fichiers, exécution de commandes, automatisation de navigateur, sous-agents, protocole MCP, et plus
- **12+ fournisseurs LLM** — OpenAI, Anthropic, Google, DeepSeek, Volcengine, Tongyi Qianwen, Zhipu, Moonshot, SiliconFlow, Baichuan, Ollama, LM Studio
- **Mode Objectif (Goal)** — Définissez un objectif et laissez l'IA planifier et exécuter des tâches multi-étapes de manière autonome
- **Plugins + Compétences + Robots** — Un système d'automatisation extensible pour créer vos propres flux de travail IA
- **Synchronisation mobile par QR Code** — Connexion directe LAN ou relais public, contrôlez votre assistant IA n'importe où
- **Prêt à l'emploi** — Téléchargez, double-cliquez et commencez. Pas d'installation CLI ni de variables d'environnement requises

---

## Aperçu de l'interface

<p align="center">
  <img src="docs/screenshot-main.png" alt="Interface principale" width="800">
</p>
<p align="center"><em>Premier lancement — Interface sombre et épurée avec gestion de projets à gauche et zone de chat au centre</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-chat.png" alt="Interface de chat" width="800">
</p>
<p align="center"><em>Chat intelligent — Sortie en streaming, changement de modèle, approbation automatique, barre d'état en un coup d'œil</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-project.png" alt="Mode projet" width="800">
</p>
<p align="center"><em>Mode Projet — Basculement double mode Chat/Objectif, sélecteur de robot, l'IA travaille dans votre répertoire de projet</em></p>

<br>

<p align="center">
  <img src="docs/screenshot-provider.png" alt="Paramètres fournisseur" width="800">
</p>
<p align="center"><em>Configuration des fournisseurs — Interface GUI visuelle pour gérer plusieurs fournisseurs LLM avec listes de modèles et tags de capacité vision</em></p>

---

## Capacités principales

| Capacité | Description |
|----------|-------------|
| Chat intelligent | Contexte multi-tours, sortie en streaming, rendu Markdown, recherche et fork de sessions |
| Appels d'outils | I/O fichiers, Shell, automatisation navigateur, sous-agents, mémoire, MCP — 40+ outils |
| Mode Objectif | Exécution multi-étapes autonome par l'IA avec contrôle de budget tokens et suivi d'état |
| Système de plugins | Browser, Computer Use, Documents, Presentations, Spreadsheets, Sites, Superpowers |
| Robots | L'IA crée automatiquement des rôles professionnels liés à des compétences et configurations de flux |
| Mobile | Serveur web intégré + WebSocket, support connexion directe LAN et relais public |
| Panneau terminal | Terminal xterm.js intégré, multi-onglets, travaillez en parallèle avec l'IA |
| Hooks | Hooks d'automatisation événementiels sur tout le cycle de vie de l'Agent |

> Pour la documentation complète et les guides, consultez le **[Guide d'utilisation](http://47.113.221.244:8081/usage.html)**

---

## Démarrage rapide

### 1. Lancer l'application

Double-cliquez sur `CN-Codex.exe` pour démarrer. Au premier lancement, un dossier `codey/` est créé dans le même répertoire.

### 2. Configurer un fournisseur

**Paramètres** → **Fournisseurs de modèles** → **Ajouter un fournisseur** → Choisir un préréglage (ex : DeepSeek, OpenAI) → Entrer la clé API et l'URL de base → **Sauvegarder** → **Activer**

### 3. Ajouter un projet et commencer

Cliquez sur le bouton **Dossier+** dans la barre latérale pour ajouter un répertoire de code, sélectionnez un modèle et commencez à discuter. L'IA exécutera toutes les opérations dans le répertoire de votre projet.

> Pour les étapes détaillées et la configuration avancée, consultez le **[Guide d'utilisation](http://47.113.221.244:8081/usage.html)**

---

## Stack technique

| Couche | Technologie |
|--------|-------------|
| Framework desktop | Tauri v2 |
| Backend | Rust (Edition 2024) |
| Frontend | React 18 + TypeScript + Vite 6 |
| Styles | Tailwind CSS 4 |
| Gestion d'état | Zustand |
| Internationalisation | react-intl |
| Base de données | SQLite |

---

## Liens

| Lien | Description |
|------|-------------|
| [Site officiel](http://47.113.221.244:8081/) | Présentation du produit et téléchargement |
| [Guide d'utilisation](http://47.113.221.244:8081/usage.html) | Documentation complète du premier lancement aux fonctionnalités avancées |
| [GitHub](https://github.com/longdream/cn-codex) | Code source et suivi des problèmes |

---

## Licence

MIT License

---

<p align="center">
  <strong>CN-Codex</strong> — Faites de l'IA votre partenaire de programmation
</p>
