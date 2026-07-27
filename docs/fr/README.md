# FaniLab-SmartContract 📦🔗

> **FaniLab** est une plateforme de logistique et d'escrow alimentée par la blockchain, conçue pour connecter les personnes et entreprises ayant besoin de transporter des marchandises avec les fournisseurs de transport disponibles. Ce référentiel contient les contrats intelligents Stellar Soroban qui alimentent le système d'escrow blockchain utilisé par la plateforme logistique.

---

## 📖 Table des matières
- [Aperçu du projet](#-aperçu-du-projet)
- [Le problème fondamental](#-le-problème-fondamental)
- [La solution](#-la-solution)
- [Comment ça marche (modèle de paiement en escrow)](#-comment-ça-marche-modèle-de-paiement-en-escrow)
- [Avantages d'inclusion financière](#-avantages-dinclusion-financière)
- [Marché cible](#-marché-cible)
- [Modèle de revenus](#-modèle-de-revenus)
- [Architecture des contrats intelligents](#-architecture-des-contrats-intelligents)
- [Pile technologique](#-pile-technologique)
- [Fonctionnalités de la plateforme](#-fonctionnalités-de-la-plateforme)
- [Phases de développement](#-phases-de-développement)
- [Structure du référentiel](#-structure-du-référentiel)
- [Instructions d'installation](#-instructions-dinstallation)
- [Instructions de déploiement du contrat](#-instructions-de-déploiement-du-contrat)
- [Variables d'environnement](#-variables-denvironnement)
- [Pipeline CI/CD](#-pipeline-cicd)
- [Directives de contribution](#-directives-de-contribution)
- [Licence](#-licence)

---

## 🌍 Aperçu du projet

FaniLab est composé de trois référentiels principaux :
1. **FaniLab-Frontend** : Stack : Next.js + TypeScript + TailwindCSS
2. **FaniLab-Backend** : Stack : Node.js + Express.js + TypeScript + MongoDB
3. **FaniLab-SmartContract** *(Ce référentiel)* : Stack : Stellar Soroban + Rust

Le référentiel de contrats intelligents alimente le **système d'escrow blockchain** utilisé par la plateforme logistique.

## ⚠️ Le problème fondamental

Les réseaux logistiques et de livraison traditionnels souffrent souvent de :
- Manque de confiance entre les expéditeurs et les livreurs indépendants.
- Frais élevés et règlements retardés pour les livreurs.
- Utilisation inefficace des actifs de transport existants.
- Difficultés pour les petits opérateurs à accéder aux économies logistiques mondiales ou transfrontalières.

## 💡 La solution

FaniLab crée une **économie logistique décentralisée partagée** en permettant aux actifs de transport existants de participer de manière sécurisée aux opérations de livraison. Les fournisseurs de transport incluent :
- Livreurs à moto
- Agents de messagerie
- Conducteurs de camionnettes
- Opérateurs de camions
- Propriétaires de transport indépendants

En tirant parti des **contrats intelligents d'escrow blockchain**, FaniLab protège les expéditeurs et les agents de livraison, garantissant que les marchandises sont transportées de manière sécurisée et les paiements sont réglés instantanément après confirmation de livraison.

## 🔄 Comment ça marche (modèle de paiement en escrow)

FaniLab assure les **transactions logistiques sans confiance** par le flux de travail suivant :

1. **Le client crée une demande de livraison** : L'expéditeur initie une commande de livraison.
2. **Le paiement est bloqué en escrow** : Le contrat intelligent sécurise le paiement.
3. **Le livreur accepte la livraison** : Un livreur est assigné à la tâche.
4. **Les marchandises sont transportées** : Le livreur accomplit le processus logistique.
5. **Le destinataire confirme la livraison** : Le destinataire vérifie l'arrivée des marchandises.
6. **Le contrat d'escrow libère le paiement au livreur** : Règlement instantané sur le réseau Stellar.

## 🤝 Avantages d'inclusion financière

La plateforme est conçue pour autonomiser les individus et les petites entreprises, en activant :
- **Livreurs indépendants**
- **Petites entreprises logistiques**
- **Opérateurs de transport ruraux**
- **Commerçants transfrontaliers**

pour participer de manière transparente à une économie logistique mondiale alimentée par la blockchain Stellar.

## 🎯 Marché cible

Notre audience cible principale comprend :
- Réseaux logistiques africains
- Commerçants PME
- Vendeurs de commerce électronique
- Startups de messagerie
- Syndicats de transport
- Opérateurs du commerce transfrontalier

## 💰 Modèle de revenus

FaniLab génère des revenus à travers les flux suivants :
- Frais de service d'escrow
- Frais de commission de livraison
- Frais de règlement transfrontalier
- Intégrations logistiques d'entreprise
- Analyses logistiques premium

## 🏗️ Architecture des contrats intelligents

Les contrats intelligents FaniLab sont l'épine dorsale du protocole logistique sans confiance. Ils sont responsables de :
- Blocage du paiement en escrow
- Libération du paiement en escrow
- Vérification de la livraison
- Règlement des transactions
- Validation de l'état de la livraison
- Métadonnées logistiques sur chaîne

### Exigences fonctionnelles

L'architecture du contrat prend en charge :
- **Création d'escrow** : Bloquer le paiement à la création d'une livraison.
- **Acceptation du livreur** : Le livreur accepte l'affectation de livraison.
- **Confirmation de livraison** : Le destinataire confirme l'arrivée du colis.
- **Libération d'escrow** : Le paiement est libéré au livreur après confirmation.
- **Gestion des litiges** : L'escrow peut être mis en pause pour résolution de litiges.

### Émission d'événements
Les contrats émettent des événements critiques pour l'indexation hors chaîne :
- `delivery_created`
- `escrow_funded`
- `driver_assigned`
- `delivery_confirmed`
- `escrow_released`

## 🛠️ Pile technologique

- **Blockchain Stellar**
- **Contrats intelligents Soroban**
- **Rust**
- **SDK Soroban**
- **CLI Stellar**
- **CLI Soroban**
- **Compilation de contrats intelligents WASM**

## ✨ Fonctionnalités de la plateforme

- Gestion d'escrow décentralisée
- Règlement instantané du livreur
- États de livraison vérifiables
- Métadonnées logistiques immuables
- Résolution de litiges sans confiance

## 🚀 Phases de développement

### Phase 1 — Contrat intelligent escrow MVP
**Focus** : Contrat intelligent minimal pour supporter les paiements de livraison basés sur l'escrow.
- Blocage du paiement en escrow
- Enregistrement de l'ID de livraison
- État de stockage d'escrow
- Mécanisme de libération de paiement

### Phase 2 — Expansion du contrat intelligent logistique
**Focus** : Suivi avancé et métadonnées d'expédition.
- Suivi de l'attribution des livreurs
- Mises à jour du statut de livraison
- Événements de confirmation de livraison
- Stockage de métadonnées d'expédition

### Phase 3 — Protocole logistique blockchain complet
**Focus** : Gouvernance décentralisée et capacités transfrontalières.
- Mécanisme de résolution de litiges
- Notation de réputation pour les livreurs
- Vérification décentralisée de livraison
- Règlement de paiement transfrontalier

## 📂 Structure du référentiel

```text
FaniLab-SmartContract/
├── contracts/
│   ├── escrow_contract/
│   │   └── lib.rs
│   ├── delivery_contract/
│   │   └── lib.rs
│   └── shared_types/
│       └── lib.rs
├── src/
│   ├── events/
│   ├── errors/
│   ├── storage/
│   └── interfaces/
├── tests/
│   ├── integration_tests/
│   └── contract_tests/
├── scripts/
│   ├── deployment/
│   ├── build/
│   ├── initialize/
│   ├── deploy-contract.sh
│   └── initialize-contract.sh
├── docs/
│   ├── architecture/
│   │   ├── smart-contract-architecture.md
│   │   └── event-system.md
│   ├── contract-design/
│   │   └── escrow-design.md
│   └── protocol/
│       └── delivery-protocol.md
├── deploy/
│   ├── testnet/
│   └── mainnet/
├── .github/
│   └── workflows/
│       └── ci.yml
├── Cargo.toml
├── Cargo.lock
├── Makefile
├── .env.example
├── README.md
├── LICENSE
├── CONTRIBUTING.md
└── SECURITY.md
```

## ⚙️ Instructions d'installation

1. **Installez Rust et les utilitaires standard :**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add wasm32-unknown-unknown
   ```

2. **Installez Stellar CLI :**
   ```bash
   cargo install --locked stellar-cli
   ```

3. **Clonez le référentiel :**
   ```bash
   git clone https://github.com/your-org/FaniLab-SmartContract.git
   cd FaniLab-SmartContract
   ```

4. **Construisez les contrats :**

   **Pour les utilisateurs Linux / macOS (utilisant Make) :**
   
   Pour construire tous les contrats :
   ```bash
   make build
   ```
   
   Pour construire des contrats spécifiques :
   ```bash
   make build-escrow
   make build-delivery
   make build-dispute
   ```
   
   Pour exécuter les tests :
   ```bash
   make test
   ```
   
   **Pour les utilisateurs Windows (ou sans Make) :**
   
   Vous pouvez exécuter les commandes `cargo` sous-jacentes directement depuis le répertoire racine :
   
   Pour construire tous les contrats :
   ```bash
   cargo build --target wasm32-unknown-unknown --release
   ```
   
   Pour construire des contrats spécifiques :
   ```bash
   cargo build -p escrow_contract --target wasm32-unknown-unknown --release
   cargo build -p delivery_contract --target wasm32-unknown-unknown --release
   cargo build -p dispute_resolution_contract --target wasm32-unknown-unknown --release
   ```
   
   Pour exécuter les tests :
   ```bash
   cargo test
   ```

## 🚢 Instructions de déploiement du contrat

1. **Configurez votre identité réseau Stellar :**
   ```bash
   stellar keys generate deployer
   ```

2. **Financez l'identité sur Testnet :**
   ```bash
   stellar keys fund deployer --network testnet
   ```

3. **Déployez le contrat Escrow :**
   ```bash
   ./scripts/deploy-contract.sh escrow_contract
   ```

4. **Initialisez le contrat :**
   ```bash
   ./scripts/initialize-contract.sh escrow_contract
   ```

## 🔑 Variables d'environnement

Copiez le fichier `.env.example` en `.env` et remplissez vos variables :

```env
STELLAR_NETWORK=testnet
SOROBAN_RPC_URL=https://soroban-testnet.stellar.org
CONTRACT_DEPLOYER_KEY=S...
NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
```

## 🔄 Pipeline CI/CD

Ce projet utilise GitHub Actions pour CI/CD. Le pipeline `.github/workflows/ci.yml` est configuré pour automatiquement :
- Exécuter les vérifications de formatage Rust (`cargo fmt`).
- Exécuter le lint Rust (`cargo clippy`).
- Compiler les contrats Soroban.
- Vérifier la construction WASM.
- Exécuter tous les tests unitaires et d'intégration.

## 📊 Statut du projet

![CI Status](https://github.com/fanilab/FaniLab-SmartContract/workflows/Rust%20CI/badge.svg)
![Security Audit](https://github.com/fanilab/FaniLab-SmartContract/workflows/Security%20Audit/badge.svg)
[![codecov](https://codecov.io/gh/fanilab/FaniLab-SmartContract/branch/main/graph/badge.svg)](https://codecov.io/gh/fanilab/FaniLab-SmartContract)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

- **Version actuelle** : 0.2.0
- **Statut d'audit** : En attente
- **Couverture de test** : > 80%
- **Réseau** : Testnet (Mainnet bientôt)

## 📚 Documentation

- [Référence API](../API.md)
- [Guide de déploiement](../DEPLOYMENT.md)
- [Liste de contrôle d'audit de sécurité](../SECURITY_AUDIT.md)
- [Guide de test](../TESTING.md)
- [Modèle de gouvernance](../GOVERNANCE.md)
- [Enregistrements de décisions architecturales](../ARCHITECTURE_DECISION_RECORDS.md)

## 🤝 Directives de contribution

Veuillez consulter notre fichier `CONTRIBUTING.md` pour les détails sur notre code de conduite et le processus de soumission des demandes d'extraction.

## 🔒 Sécurité

La sécurité est notre priorité absolue. Veuillez consulter [SECURITY.md](../SECURITY.md) pour notre politique de sécurité et notre processus de signalement des vulnérabilités.

**Bug Bounty** : Nous offrons des récompenses jusqu'à $50 000 pour les découvertes de sécurité critiques.

## 📜 Licence

Ce projet est autorisé sous la licence MIT - voir le fichier `LICENSE` pour les détails.

## 🌟 Remerciements

- Stellar Development Foundation pour Soroban
- Les communautés Rust et Stellar
- Tous nos contributeurs et supporters

## 📞 Contact & Communauté

- **Site Web** : https://fanilab.com
- **Email** : contact@fanilab.com
- **Twitter** : [@FaniLabHQ](https://twitter.com/FaniLabHQ)
- **Discord** : [Rejoignez notre communauté](https://discord.gg/fanilab)
- **GitHub** : [Organisation FaniLab](https://github.com/fanilab)

---

Construit avec ❤️ par l'équipe FaniLab | Alimenté par Stellar Soroban
