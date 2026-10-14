import { Routes } from '@angular/router';

import { DownloadsPageComponent } from './features/downloads/downloads-page.component';
import { SettingsPageComponent } from './features/settings/settings-page.component';

/** Hash-based routes (see `app.config.ts`): the list is home, settings is one level deep. */
export const routes: Routes = [
  { path: '', component: DownloadsPageComponent, pathMatch: 'full' },
  { path: 'settings', component: SettingsPageComponent },
  { path: '**', redirectTo: '' },
];
