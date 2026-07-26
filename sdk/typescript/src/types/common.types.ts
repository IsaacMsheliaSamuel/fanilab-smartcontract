/**
 * Common types shared across FaniLab smart contracts
 */

export enum EscrowStatus {
  Locked = 'Locked',
  Holdback = 'Holdback',
  Released = 'Released',
  Refunded = 'Refunded',
  Split = 'Split',
}

export enum DeliveryStatus {
  Pending = 'Pending',
  Active = 'Active',
  InTransit = 'InTransit',
  Delivered = 'Delivered',
  Disputed = 'Disputed',
  Cancelled = 'Cancelled',
}

export interface ProtocolConfig {
  token: string;
  platformFeeBps: number;
  protocolVersion: number;
  slippageToleranceBps: number;
}

export interface EscrowRecord {
  sender: string;
  recipient: string;
  driver: string;
  token: string;
  amount: bigint;
  status: EscrowStatus;
  createdAt: number;
  expiresAt?: number;
  disputedBy?: string;
  disputedAt?: number;
  fleetId?: number;
}

export interface PartyAddresses {
  sender: string;
  driver: string;
  recipient: string;
}

export interface FaniLabError {
  code: number;
  message: string;
}

export const ErrorCodes = {
  Unauthorized: 1,
  AlreadyInitialized: 2,
  NotInitialized: 3,
  DeliveryNotFound: 4,
  InvalidState: 5,
  InsufficientFunds: 6,
  EscrowLocked: 7,
  DuplicateDelivery: 8,
  ProviderNotFound: 9,
  InvalidAddress: 10,
  ProtocolPaused: 11,
};

export interface ContractInvokeOptions {
  networkPassphrase?: string;
  serverUrl?: string;
  timeout?: number;
}
