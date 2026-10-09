export type ThemeMode = 'light' | 'dark' | 'system';
export type Accent = 'citron' | 'periwinkle';
export type Density = 'comfortable' | 'compact';
export type Rsvp = 'yes' | 'maybe' | 'no';
export interface Conversation { id: string; name: string; group: string; unread?: number; muted?: boolean }
export interface Person { id: string; name: string; detail?: string }
export const themes = {
  light: { canvas:'#faf9f5', chrome:'#f0f0e8', surface:'#ffffff', text:'#191b18', muted:'#575b53', line:'#868c7d', rule:'#191b18', wash:'#e5e8dc', accent:'#d9ed70' },
  dark: { canvas:'#191b18', chrome:'#141612', surface:'#22261f', text:'#ebece1', muted:'#adb2a5', line:'#747c68', rule:'#747c68', wash:'#30362a', accent:'#d9ed70' }
} as const;
export const rsvpOptions = [{value:'yes',label:'Eg kjem'},{value:'maybe',label:'Kanskje'},{value:'no',label:'Kan ikkje'}] as const;
