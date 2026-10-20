import { Routes } from '@angular/router';

import { DownloadsPageComponent } from './features/downloads/downloads-page.component';

/** Hash-based routes (see `app.config.ts`): the list is home, settings is one level deep. */
export const routes: Routes = [
  { path: '', component: DownloadsPageComponent, pathMatch: 'full' },
  { path: 'settings', loadComponent: () => import('./features/settings/settings-page.component').then((m) => m.SettingsPageComponent) },
  { path: 'research', loadComponent: () => import('./features/research/research-page.component').then((m) => m.ResearchPageComponent) },
  { path: '**', redirectTo: '' },
];
