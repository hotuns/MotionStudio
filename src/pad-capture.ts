import {buttonNames, inputs, type Status} from './types';

const directions = [
  ['left_stick_left', 'left_stick_right'], ['left_stick_up', 'left_stick_down'],
  ['right_stick_left', 'right_stick_right'], ['right_stick_up', 'right_stick_down'],
];

export function heldPadInputs(status: Status) {
  const held = new Set<string>(buttonNames.filter((_, i) => status.buttons[i]));
  status.axes.forEach((value, i) => {
    if (i < 4 && Math.abs(value) > 0.4) held.add(directions[i][value > 0 ? 1 : 0]);
    else if (i >= 4 && value > 0.4) held.add(i === 4 ? 'left_trigger' : 'right_trigger');
  });
  const touches = status.touches ?? [];
  const contacts = (status.device?.touchpads ?? 0) >= 2 ? touches.map(pad => pad.find(contact => contact.active)) : touches[0] ?? [];
  contacts.forEach((contact, i) => { if (contact?.active) held.add('touch' + (i + 1)); });
  return held;
}

// Mirror the engine's release gate for recording controls.
export const padInputNeutral = (status: Status) => !status.buttons.some(Boolean) &&
  !status.touches?.some(pad => pad.some(contact => contact.active)) &&
  status.axes.every((value, i) => i < 4 ? Math.abs(value) <= 0.25 : value <= 0.1);

export type GamepadBindingRecording = {
  wholeStick: boolean;
  waitingForNeutral: boolean;
  preview?: string[];
};

export function recordedSticks(observed: string[]) {
  return observed.flatMap(input => {
    const stick = /^(left_stick|right_stick)_(?:up|down|left|right)$/.exec(input)?.[1];
    return stick ? [stick] : [];
  });
}

export type GyroBindingField = 'activation' | 'pause_input';
export type GyroBindingRecording = {
  gyroField: GyroBindingField;
  waitingForNeutral: boolean;
  preview?: string[];
  issue?: 'multiple' | 'duplicate';
};

export function padBindingCandidates(observed: string[], split: boolean): string[] {
  const candidates = new Set(observed.filter(input => inputs.includes(input)));
  // Pressing a pad usually also touches its surface. Suppress only contacts on
  // that pad; a shoulder button or a contact on the other pad is still distinct.
  if (candidates.has('touchpad')) {
    candidates.delete('touch1');
    if (!split) candidates.delete('touch2');
  }
  if (split && candidates.has('misc2')) candidates.delete('touch2');
  return [...candidates];
}
