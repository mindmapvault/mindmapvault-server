import { ApiError } from './client';

export type Subscription = {
  plan?: string;
  status?: string;
  current_limit_bytes?: number;
  current_period_end?: string;
  plan_source?: string;
  manual_override_active?: boolean;
  stripe_ready?: boolean;
  publishable_key?: string;
  paid_yearly_price_label?: string;
  paid_yearly_limit_bytes?: number;
  max_attachment_size_bytes?: number;
  max_active_shares?: number;
  can_create_public_shares?: boolean;
  can_include_attachments_in_shares?: boolean;
  can_use_plaintext_collaboration?: boolean;
  can_export_large_maps?: boolean;
  can_use_admin_controls?: boolean;
};

function disabled(): never {
  throw new ApiError(501, 'Subscription endpoints are disabled in community server mode', 'subscription_disabled');
}

export async function getBillingConfig(): Promise<Subscription> {
  return Promise.reject(disabled());
}

export async function getProfileSubscription(): Promise<Subscription> {
  return Promise.reject(disabled());
}

export async function getCapabilities(): Promise<Subscription> {
  return Promise.reject(disabled());
}

export async function getSubscription(): Promise<Subscription | null> {
  return Promise.reject(disabled());
}

export async function createCheckoutSession(): Promise<{ url: string; session_id?: string }> {
  return Promise.reject(disabled());
}

export async function confirmCheckoutSession(sessionId: string): Promise<{ subscription_tier: string; stripe_subscription_status?: string }> {
  void sessionId;
  return Promise.reject(disabled());
}

export async function createPortalSession(): Promise<{ url: string }> {
  return Promise.reject(disabled());
}

export default { getSubscription, getBillingConfig, getProfileSubscription, getCapabilities, createCheckoutSession, confirmCheckoutSession, createPortalSession };
