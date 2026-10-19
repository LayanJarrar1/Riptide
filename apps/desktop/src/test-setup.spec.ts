// Spec setup (a spec file so it shares the module instances of the specs): Arabic is a lazy chunk in
// the app; specs switch languages synchronously, so it is registered up front here.
import ar from './i18n/ar.json';
import { registerTable } from './app/i18n/i18n.service';

registerTable('ar', ar);
