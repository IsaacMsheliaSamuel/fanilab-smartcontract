# FaniLab-SmartContract 📦🔗

> **FaniLab** é uma Plataforma de Logística e Depósito em Caução com Blockchain, projetada para conectar indivíduos e empresas que precisam transportar mercadorias com provedores de transporte disponíveis. Este repositório contém os contratos inteligentes Stellar Soroban que alimentam o sistema de depósito em caução blockchain usado pela plataforma de logística.

---

## 📖 Índice
- [Visão Geral do Projeto](#-visão-geral-do-projeto)
- [O Problema Fundamental](#-o-problema-fundamental)
- [A Solução](#-a-solução)
- [Como Funciona (Modelo de Pagamento em Depósito)](#-como-funciona-modelo-de-pagamento-em-depósito)
- [Benefícios de Inclusão Financeira](#-benefícios-de-inclusão-financeira)
- [Mercado Alvo](#-mercado-alvo)
- [Modelo de Receita](#-modelo-de-receita)
- [Arquitetura de Contrato Inteligente](#-arquitetura-de-contrato-inteligente)
- [Pilha Tecnológica](#-pilha-tecnológica)
- [Recursos da Plataforma](#-recursos-da-plataforma)
- [Fases de Desenvolvimento](#-fases-de-desenvolvimento)
- [Estrutura do Repositório](#-estrutura-do-repositório)
- [Instruções de Instalação](#-instruções-de-instalação)
- [Instruções de Implantação de Contrato](#-instruções-de-implantação-de-contrato)
- [Variáveis de Ambiente](#-variáveis-de-ambiente)
- [Pipeline CI/CD](#-pipeline-cicd)
- [Diretrizes de Contribuição](#-diretrizes-de-contribuição)
- [Licença](#-licença)

---

## 🌍 Visão Geral do Projeto

FaniLab é composto por três repositórios principais:
1. **FaniLab-Frontend**: Stack: Next.js + TypeScript + TailwindCSS
2. **FaniLab-Backend**: Stack: Node.js + Express.js + TypeScript + MongoDB
3. **FaniLab-SmartContract** *(Este Repositório)*: Stack: Stellar Soroban + Rust

O repositório de contrato inteligente alimenta o **sistema de depósito em caução blockchain** usado pela plataforma de logística.

## ⚠️ O Problema Fundamental

As redes tradicionais de logística e entrega frequentemente sofrem com:
- Falta de confiança entre remetentes e motoristas de entrega independentes.
- Taxas altas e liquidações atrasadas para motoristas.
- Utilização ineficiente dos ativos de transporte existentes.
- Dificuldades para pequenos operadores acessarem economias de logística global ou transfronteiriça.

## 💡 A Solução

FaniLab cria uma **economia logística descentralizada compartilhada** permitindo que ativos de transporte existentes participem com segurança de operações de entrega. Os provedores de transporte incluem:
- Motoboys de entrega
- Agentes de correio
- Motoristas de van
- Operadores de caminhão
- Proprietários de transporte independentes

Aproveitando os **contratos inteligentes de depósito em caução blockchain**, FaniLab protege remetentes e agentes de entrega, garantindo que as mercadorias sejam transportadas com segurança e os pagamentos sejam liquidados instantaneamente após a confirmação da entrega.

## 🔄 Como Funciona (Modelo de Pagamento em Depósito)

FaniLab garante **transações logísticas sem confiança** através do seguinte fluxo de trabalho:

1. **Cliente cria solicitação de entrega**: O remetente inicia um pedido de entrega.
2. **Pagamento é bloqueado em depósito**: O contrato inteligente mantém o pagamento com segurança.
3. **Motorista aceita a entrega**: Um motorista é atribuído à tarefa.
4. **Mercadorias são transportadas**: O motorista cumpre o processo logístico.
5. **Destinatário confirma a entrega**: O destinatário verifica a chegada das mercadorias.
6. **Contrato de depósito libera pagamento para o motorista**: Liquidação instantânea na rede Stellar.

## 🤝 Benefícios de Inclusão Financeira

A plataforma foi construída para capacitar indivíduos e pequenas empresas, permitindo:
- **Motoristas de entrega independentes**
- **Pequenas empresas de logística**
- **Operadores de transporte rural**
- **Comerciantes transfronteiriços**

para participar perfeitamente de uma economia logística global alimentada pelo blockchain Stellar.

## 🎯 Mercado Alvo

Nossa audiência-alvo principal inclui:
- Redes de logística africana
- Comerciantes PME
- Vendedores de comércio eletrônico
- Startups de correio
- Sindicatos de transporte
- Operadores de comércio transfronteiriço

## 💰 Modelo de Receita

FaniLab gera receita através dos seguintes fluxos:
- Taxas de serviço de depósito
- Taxas de comissão de entrega
- Taxas de liquidação transfronteiriça
- Integrações logísticas corporativas
- Análise de logística premium

## 🏗️ Arquitetura de Contrato Inteligente

Os contratos inteligentes FaniLab são a espinha dorsal do protocolo logístico sem confiança. Eles são responsáveis por:
- Bloqueio de pagamento em depósito
- Liberação de pagamento em depósito
- Verificação de entrega
- Liquidação de transação
- Validação de estado de entrega
- Metadados de logística na cadeia

### Requisitos Funcionais

A arquitetura do contrato suporta:
- **Criação de Depósito**: Bloquear pagamento quando uma entrega é criada.
- **Aceitação do Motorista**: O motorista aceita a atribuição de entrega.
- **Confirmação de Entrega**: O destinatário confirma a chegada da encomenda.
- **Liberação de Depósito**: O pagamento é liberado para o motorista após confirmação.
- **Tratamento de Disputas**: O depósito pode ser pausado para resolução de disputas.

### Emissão de Eventos
Os contratos emitem eventos críticos para indexação fora da cadeia:
- `delivery_created`
- `escrow_funded`
- `driver_assigned`
- `delivery_confirmed`
- `escrow_released`

## 🛠️ Pilha Tecnológica

- **Blockchain Stellar**
- **Contratos Inteligentes Soroban**
- **Rust**
- **SDK Soroban**
- **CLI Stellar**
- **CLI Soroban**
- **Compilação de contrato inteligente WASM**

## ✨ Recursos da Plataforma

- Gerenciamento de Depósito Descentralizado
- Liquidação Instantânea do Motorista
- Estados de Entrega Verificáveis
- Metadados de Logística Imutáveis
- Resolução de Disputas sem Confiança

## 🚀 Fases de Desenvolvimento

### Fase 1 — Contrato Inteligente MVP de Depósito
**Foco:** Contrato inteligente mínimo para suportar pagamentos de entrega baseados em depósito.
- Bloqueio de pagamento em depósito
- Registro de ID de entrega
- Estado de armazenamento de depósito
- Mecanismo de liberação de pagamento

### Fase 2 — Expansão de Contrato Inteligente de Logística
**Foco:** Rastreamento avançado e metadados de remessa.
- Rastreamento de atribuição de motorista
- Atualizações de status de entrega
- Eventos de confirmação de entrega
- Armazenamento de metadados de remessa

### Fase 3 — Protocolo Logístico Blockchain Completo
**Foco:** Governança descentralizada e capacidades transfronteiriças.
- Mecanismo de resolução de disputas
- Pontuação de reputação para motoristas
- Verificação de entrega descentralizada
- Liquidação de pagamento transfronteiriço

## 📂 Estrutura do Repositório

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

## ⚙️ Instruções de Instalação

1. **Instale Rust e utilitários padrão:**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   rustup target add wasm32-unknown-unknown
   ```

2. **Instale Stellar CLI:**
   ```bash
   cargo install --locked stellar-cli
   ```

3. **Clone o repositório:**
   ```bash
   git clone https://github.com/your-org/FaniLab-SmartContract.git
   cd FaniLab-SmartContract
   ```

4. **Construa os contratos:**

   **Para Usuários Linux / macOS (usando Make):**
   
   Para construir todos os contratos:
   ```bash
   make build
   ```
   
   Para construir contratos específicos:
   ```bash
   make build-escrow
   make build-delivery
   make build-dispute
   ```
   
   Para executar testes:
   ```bash
   make test
   ```
   
   **Para Usuários Windows (ou sem Make):**
   
   Você pode executar os comandos `cargo` subjacentes diretamente do diretório raiz:
   
   Para construir todos os contratos:
   ```bash
   cargo build --target wasm32-unknown-unknown --release
   ```
   
   Para construir contratos específicos:
   ```bash
   cargo build -p escrow_contract --target wasm32-unknown-unknown --release
   cargo build -p delivery_contract --target wasm32-unknown-unknown --release
   cargo build -p dispute_resolution_contract --target wasm32-unknown-unknown --release
   ```
   
   Para executar testes:
   ```bash
   cargo test
   ```

## 🚢 Instruções de Implantação de Contrato

1. **Configure sua identidade de rede Stellar:**
   ```bash
   stellar keys generate deployer
   ```

2. **Financie a identidade no Testnet:**
   ```bash
   stellar keys fund deployer --network testnet
   ```

3. **Implante o contrato Escrow:**
   ```bash
   ./scripts/deploy-contract.sh escrow_contract
   ```

4. **Inicialize o contrato:**
   ```bash
   ./scripts/initialize-contract.sh <CONTRACT_ID>
   ```

## 🔑 Variáveis de Ambiente

Copie o arquivo `.env.example` para `.env` e preencha suas variáveis:

```env
STELLAR_NETWORK=testnet
SOROBAN_RPC_URL=https://soroban-testnet.stellar.org
CONTRACT_DEPLOYER_KEY=S...
NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
```

## 🔄 Pipeline CI/CD

Este projeto usa GitHub Actions para CI/CD. O pipeline `.github/workflows/ci.yml` é configurado para automaticamente:
- Executar verificações de formatação Rust (`cargo fmt`).
- Executar linting Rust (`cargo clippy`).
- Compilar os contratos Soroban.
- Verificar a compilação WASM.
- Executar todos os testes unitários e de integração.

## 📊 Status do Projeto

![CI Status](https://github.com/fanilab/FaniLab-SmartContract/workflows/Rust%20CI/badge.svg)
![Security Audit](https://github.com/fanilab/FaniLab-SmartContract/workflows/Security%20Audit/badge.svg)
[![codecov](https://codecov.io/gh/fanilab/FaniLab-SmartContract/branch/main/graph/badge.svg)](https://codecov.io/gh/fanilab/FaniLab-SmartContract)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

- **Versão Atual**: 0.2.0
- **Status da Auditoria**: Pendente
- **Cobertura de Teste**: > 80%
- **Rede**: Testnet (Mainnet em breve)

## 📚 Documentação

- [Referência de API](../API.md)
- [Guia de Implantação](../DEPLOYMENT.md)
- [Lista de Verificação de Auditoria de Segurança](../SECURITY_AUDIT.md)
- [Guia de Testes](../TESTING.md)
- [Modelo de Governança](../GOVERNANCE.md)
- [Registros de Decisão de Arquitetura](../ARCHITECTURE_DECISION_RECORDS.md)

## 🤝 Diretrizes de Contribuição

Por favor, consulte nosso arquivo `CONTRIBUTING.md` para detalhes sobre nosso código de conduta e o processo de envio de pull requests.

## 🔒 Segurança

A segurança é nossa prioridade máxima. Por favor, consulte [SECURITY.md](../SECURITY.md) para nossa política de segurança e processo de relatório de vulnerabilidades.

**Bug Bounty**: Oferecemos recompensas até $50.000 por descobertas críticas de segurança.

## 📜 Licença

Este projeto está licenciado sob a Licença MIT - consulte o arquivo `LICENSE` para detalhes.

## 🌟 Agradecimentos

- Stellar Development Foundation por Soroban
- As comunidades Rust e Stellar
- Todos os nossos contribuidores e apoiadores

## 📞 Contato & Comunidade

- **Website**: https://fanilab.com
- **Email**: contact@fanilab.com
- **Twitter**: [@FaniLabHQ](https://twitter.com/FaniLabHQ)
- **Discord**: [Junte-se à nossa comunidade](https://discord.gg/fanilab)
- **GitHub**: [Organização FaniLab](https://github.com/fanilab)

---

Construído com ❤️ pela Equipe FaniLab | Alimentado por Stellar Soroban
