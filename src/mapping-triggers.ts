import {chordParts, type GamepadBinding, type Profile, type TriggerSettings} from './types';
import layout from './virtual-gamepad-layout.json';

export type TriggerBinding = TriggerSettings & {input:string;threshold?:number};
export const hasAnalogCondition = (input:string) => chordParts(input)
  .some(part => /^(left|right)_trigger$|^(left|right)_stick_(up|down|left|right)$/.test(part));

export function isAnalogGamepadBinding(binding:GamepadBinding) {
  return layout.sticks.includes(binding.output)
    || layout.triggers.includes(binding.output) && layout.triggers.includes(binding.input.trim());
}

export function triggerPeers(profile:Profile|undefined, kind:'action'|'gamepad', index:number|null):TriggerBinding[] {
  if (!profile) return [];
  return [
    ...profile.actions.filter((_,i) => kind !== 'action' || i !== index),
    ...(profile.virtual_gamepad === 'none' ? [] : (profile.gamepad_bindings ?? [])
      .filter((binding,i) => (kind !== 'gamepad' || i !== index) && !isAnalogGamepadBinding(binding)
        && (profile.virtual_gamepad === 'dualshock_4' || binding.output !== 'touchpad'))),
  ];
}

// UI hints mirror the engine's canonical chord + analog threshold condition.
export function triggerCompetition(binding:TriggerBinding, peers:TriggerBinding[]) {
  const key = chordParts(binding.input).sort().join('+');
  const group = [binding, ...(binding.allow_simultaneous ? [] : peers.filter(other =>
    !other.allow_simultaneous && chordParts(other.input).sort().join('+') === key
    && (!hasAnalogCondition(binding.input) || (other.threshold ?? 0.5) === (binding.threshold ?? 0.5))))];
  const windows = (mode:TriggerSettings['trigger']) => group
    .filter(other => other.trigger === mode).map(other => other.trigger_ms ?? 250);
  const short = windows('short'), long = windows('long'), double = windows('double');
  return {short:short.length ? Math.max(...short) : null,
    long:long.length ? Math.min(...long) : null, double:double.length ? Math.max(...double) : null};
}
