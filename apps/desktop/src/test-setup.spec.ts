// Spec setup (a spec file so it shares the module instances of the specs): both languages are lazy
// chunks in the app; specs read and switch languages synchronously, so they are registered up front.
import ar from './i18n/ar.json';
import en from './i18n/en.json';
import { registerTable } from './app/i18n/i18n.service';

registerTable('ar', ar);
registerTable('en', en);
