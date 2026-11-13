import {buttonNames, inputs, labels, swipeInputs, triggerStageInputs} from '../types';

export const imuOptions:[string,string,number][]=[
  ['imu_tilt_left','向左倾斜',45],['imu_tilt_right','向右倾斜',45],['imu_tilt_up','抬高手柄前端',45],['imu_tilt_down','压低手柄前端',45],
  ['imu_tilt_pitch','俯仰角（双向）',45],['imu_tilt_roll','横滚角（双向）',45],
  ['imu_gyro_pitch','俯仰转速（双向）',360],['imu_gyro_pitch_up','向上转动',360],['imu_gyro_pitch_down','向下转动',360],
  ['imu_gyro_yaw','水平转速（双向）',360],['imu_gyro_yaw_left','向左转动',360],['imu_gyro_yaw_right','向右转动',360],
  ['imu_gyro_roll','横滚转速（双向）',360],['imu_gyro_roll_left','向左横滚',360],['imu_gyro_roll_right','向右横滚',360],
  ['imu_accel_ud','上下加速度（双向）',1],['imu_accel_up','向上加速',1],['imu_accel_down','向下加速',1],
  ['imu_accel_lr','左右加速度（双向）',1],['imu_accel_left','向左加速',1],['imu_accel_right','向右加速',1],
  ['imu_accel_fb','前后加速度（双向）',1],['imu_accel_forward','向前加速',1],['imu_accel_backward','向后加速',1],
];

export const controllerCategories = [
  {id:'buttons', label:'按键'},
  {id:'dpad', label:'十字键'},
  {id:'triggers', label:'扳机键'},
  {id:'sticks', label:'摇杆'},
  {id:'motion', label:'陀螺仪'},
  {id:'touchpad', label:'触摸板'},
] as const;
export type ControllerCategory = typeof controllerCategories[number]['id'];

export function controllerCategory(input: string, split = false): ControllerCategory {
  if (input.startsWith('imu_')) return 'motion';
  if (input.startsWith('touch') || split && input === 'misc2') return 'touchpad';
  if (input.startsWith('dpad_')) return 'dpad';
  if (/^(left|right)_trigger/.test(input)) return 'triggers';
  if (/^(left|right)_stick/.test(input)) return 'sticks';
  return 'buttons';
}

const axisLabels: Record<string,string> = {
  left_stick_x:'左摇杆水平', left_stick_y:'左摇杆垂直',
  right_stick_x:'右摇杆水平', right_stick_y:'右摇杆垂直',
};

// Native motion-output pin names differ from ordinary gamepad binding names.
export const motionPadOutputs: Record<string,string> = {
  left_stick_x:'left_stick_x', left_stick_y:'left_stick_y',
  right_stick_x:'right_stick_x', right_stick_y:'right_stick_y',
  left_trigger:'left_trigger', right_trigger:'right_trigger',
  btn_south:'south', btn_east:'east', btn_west:'west', btn_north:'north',
  btn_start:'start', btn_back:'back', btn_ls:'left_stick_click', btn_rs:'right_stick_click',
  btn_lb:'left_shoulder', btn_rb:'right_shoulder', btn_guide:'guide',
  dpad_up:'dpad_up', dpad_down:'dpad_down', dpad_left:'dpad_left', dpad_right:'dpad_right',
};

export const padInputLabel = (input: string, split = false): string =>
  split && input === 'touchpad' ? '触摸板 1 按下' :
  split && input === 'misc2' ? '触摸板 2 按下' :
  input === 'left_stick' ? '左摇杆' : input === 'right_stick' ? '右摇杆' :
  axisLabels[input] ?? imuOptions.find(([id]) => id === input)?.[1] ?? labels[input] ?? input;

// One ordering for compact buttons, searchable pickers and virtual pad outputs.
// Each caller supplies its supported signals; motion signals are not digital chords.
const controllerOrder = [...new Set([
  ...buttonNames,
  'left_trigger', ...triggerStageInputs.filter(id => id.startsWith('left_')),
  'right_trigger', ...triggerStageInputs.filter(id => id.startsWith('right_')),
  ...['left','right'].flatMap(side => [side+'_stick', side+'_stick_click',
    ...['up','down','left','right','inner','outer'].map(part => side+'_stick_'+part)]),
  'touchpad', 'touch1', 'touch2', ...swipeInputs,
  ...imuOptions.map(([id]) => id), ...inputs,
])];

export function controllerGroups(allowed: readonly string[], split = false) {
  const order = [...controllerOrder, ...allowed.filter(id => !controllerOrder.includes(id))];
  return controllerCategories.map(category => ({...category,
    values: order.filter(id => allowed.includes(id) && controllerCategory(id, split) === category.id),
  })).filter(group => group.values.length > 0);
}
