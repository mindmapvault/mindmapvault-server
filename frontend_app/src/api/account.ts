import { api } from './client';
import type {
  NotificationEvent,
  UpdateUserAccountSettingsRequest,
  UpdateUserNotificationSettingsRequest,
  UserAccountSettings,
  UserNotificationSettings,
} from '../types';

export type NotificationStateFilter = 'all' | 'unread' | 'saved' | 'done';

export const accountApi = {
  getSettings: () => api.get<UserAccountSettings>('/auth/settings'),
  updateSettings: (body: UpdateUserAccountSettingsRequest) =>
    api.patch<UserAccountSettings>('/auth/settings', body),
  getNotificationSettings: () => api.get<UserNotificationSettings>('/notifications/settings'),
  updateNotificationSettings: (body: UpdateUserNotificationSettingsRequest) =>
    api.patch<UserNotificationSettings>('/notifications/settings', body),
  listNotifications: (state: NotificationStateFilter = 'all', limit = 8) =>
    api.get<NotificationEvent[]>(`/notifications?state=${encodeURIComponent(state)}&limit=${limit}`),
  markNotificationRead: (id: string, value: boolean) =>
    api.patch<{ updated: boolean }>(`/notifications/${encodeURIComponent(id)}/read`, { value }),
  markNotificationSaved: (id: string, value: boolean) =>
    api.patch<{ updated: boolean }>(`/notifications/${encodeURIComponent(id)}/saved`, { value }),
  markNotificationDone: (id: string, value: boolean) =>
    api.patch<{ updated: boolean }>(`/notifications/${encodeURIComponent(id)}/done`, { value }),
  markAllNotificationsRead: () =>
    api.post<{ updated: number }>('/notifications/mark-all-read', {}),
};

export default accountApi;