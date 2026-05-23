import type { StorageAdapter } from '../../storage';

export const CONNECTOR_API_VERSION = '0.1.0';

export type DeploymentTarget = 'foss' | 'server' | 'saas' | 'enterprise';

export type StorageConnector = StorageAdapter;

export interface UserIdentity {
  username: string | null;
  authenticated: boolean;
}

export interface AuthCapabilities {
  canRegister: boolean;
  canChangePassword: boolean;
  hasSso: boolean;
  supportsOfflineUnlock: boolean;
}

export interface AuthConnector {
  capabilities: AuthCapabilities;
  getIdentity(): UserIdentity;
  isAuthenticated(): boolean;
  logout(): void;
}

export interface BillingConnector {
  getPlan(): 'free' | 'paid' | 'enterprise';
  isFeatureEnabled(feature: string): boolean;
  openUpgradeFlow(): void;
}

export interface CollaborationConnector {
  enabled: boolean;
}

export interface FeatureFlagConnector {
  hasFeature(feature: string): boolean;
}

export interface TelemetryConnector {
  track(event: string, payload?: Record<string, unknown>): void;
}

export interface ConnectorRegistry {
  target: DeploymentTarget;
  storage: StorageConnector;
  auth: AuthConnector;
  billing: BillingConnector;
  collaboration: CollaborationConnector;
  features: FeatureFlagConnector;
  telemetry: TelemetryConnector;
}
