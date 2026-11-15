import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';
import {consentPage} from './consent_preferences.js';

export default class PrimaryPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        window.add(consentPage(this.getSettings()));
    }
}
