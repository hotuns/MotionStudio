import { buttonNames, type Device, type Status } from '../types';
import styles from './ControllerStatusSvg.module.css';

type ControllerStatusSvgProps = {
  bindingCounts?: ReadonlyMap<string, number>
  device?: Device | null
  status?: Status | null
  selectedControl?: string | null
  onSelectControl?: (control: string) => void
}

type SharedControlProps = {
  bound?: boolean
  onSelect?: () => void
  pressed?: boolean
  selected?: boolean
  title?: string
}

type ButtonBubbleProps = SharedControlProps & {
  cx: number
  cy: number
  label: string
  radius?: number
}

type PathButtonProps = SharedControlProps & {
  compact?: boolean
  d: string
  label: string
  labelX: number
  labelY: number
}

type StickProps = SharedControlProps & {
  cx: number
  cy: number
  x: number
  y: number
}

type TriggerPathProps = SharedControlProps & {
  d: string
  fillX: number
  fillY: number
  fillWidth: number
  label: string
  labelX: number
  labelY: number
  value: number
}

type PaddleButtonProps = SharedControlProps & {
  height: number
  label: string
  width: number
  x: number
  y: number
}

const PADDLE_COMMANDS = ['left_paddle1', 'left_paddle2', 'right_paddle1', 'right_paddle2'] as const

type PaddleCommand = (typeof PADDLE_COMMANDS)[number]

type PaddleLayoutEntry = {
  command: PaddleCommand
  height: number
  width: number
  x: number
  y: number
}

const PADDLE_LAYOUT: PaddleLayoutEntry[] = [
  { command: 'left_paddle1', x: 162, y: 656, width: 132, height: 38 },
  { command: 'left_paddle2', x: 196, y: 710, width: 132, height: 38 },
  { command: 'right_paddle1', x: 823, y: 656, width: 132, height: 38 },
  { command: 'right_paddle2', x: 789, y: 710, width: 132, height: 38 },
]

const BINDING_BADGES: ReadonlyArray<readonly [string, number, number]> = [
  ['left_trigger', 326, 37], ['right_trigger', 797, 37],
  ['left_shoulder', 345, 106], ['right_shoulder', 778, 106],
  ['left_stick', 347, 244], ['right_stick', 753, 397],
  ['dpad_up', 424, 413], ['dpad_left', 373, 456], ['dpad_right', 474, 456], ['dpad_down', 424, 502],
  ['north', 855, 202], ['east', 927, 326], ['south', 854, 392], ['west', 734, 327],
  ['back', 461, 319], ['start', 664, 319], ['guide', 586, 179],
  ['misc1', 608, 352], ['misc2', 617, 420], ['touchpad', 628, 236],
  ...PADDLE_LAYOUT.map(({command, x, y, width, height}) => [command, x + width - 10, y + height / 2] as const),
]

const BACK_INPUT_TITLES: Record<PaddleCommand, string> = {
  left_paddle1: 'Primary left back paddle / Joy-Con L SL',
  left_paddle2: 'Secondary left back paddle / Joy-Con L SR',
  right_paddle1: 'Primary right back paddle / Joy-Con R SR',
  right_paddle2: 'Secondary right back paddle / Joy-Con R SL',
}

// Unbranded front-view line artwork; coordinates retain the live input hit targets.
const GENERIC_PATHS = {
  shell: 'M184 140 C169 149 112 292 70 452 C41 557 58 622 112 650 C139 664 171 663 194 649 L324 576 Q339 567 359 567 H753 Q772 567 789 576 L919 649 C958 674 1002 653 1030 622 C1070 577 1080 539 1064 473 C1038 354 992 211 942 141 C932 127 919 119 901 113 L767 87 L737 104 H394 L359 84 C300 86 235 104 184 140 Z',
  leftGrip: 'M184 140 C231 122 296 121 359 120 H755 C822 120 884 125 940 143',
  rightGrip: 'M418 104 L414 120 M705 104 L707 120 M469 109 H491 M499 107 V113 M506 107 V113 M513 107 V113',
  leftTrigger: 'M204 104 L240 40 Q250 27 277 27 Q291 27 298 40 L314 72 C269 77 234 88 204 104 Z',
  rightTrigger: 'M813 72 L828 39 Q834 24 855 26 Q881 27 890 40 L925 104 C890 87 852 78 813 72 Z',
  leftBumper: 'M185 138 Q210 86 359 67 L393 88 H418 L414 120 H394 L359 94 Q256 94 194 146 Q179 154 185 138 Z',
  rightBumper: 'M705 88 H737 L767 67 Q890 73 935 131 Q948 149 932 147 Q873 108 767 94 L738 120 H707 Z',
  dpadUp: 'M399 432 V404 Q399 393 411 392 H437 Q449 393 449 404 V432 Z',
  dpadDown: 'M399 480 H449 V508 Q449 519 437 520 H411 Q399 519 399 508 Z',
  dpadLeft: 'M399 432 H371 Q349 432 348 444 V468 Q349 480 371 480 H399 Z',
  dpadRight: 'M449 432 H477 Q499 432 500 444 V468 Q499 480 477 480 H449 Z',
  view: 'M482 276 A24 24 0 1 1 482 324 A24 24 0 1 1 482 276 Z',
  menu: 'M642 276 A24 24 0 1 1 642 324 A24 24 0 1 1 642 276 Z',
  touchpad: 'M516 232 H608 Q619 232 619 243 V259 Q619 270 608 270 H516 Q505 270 505 259 V243 Q505 232 516 232 Z',
  share: 'M545 339 H579 Q591 339 591 350 V358 Q591 369 579 369 H545 Q533 369 533 358 V350 Q533 339 545 339 Z',
  mic: 'M539 406 H585 Q597 406 597 418 V424 Q597 436 585 436 H539 Q527 436 527 424 V418 Q527 406 539 406 Z',
} as const

const join = (...parts: Array<string | false | null | undefined>) => parts.filter(Boolean).join(' ')

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value))

const paddleLabels: Record<PaddleCommand, string> = {
  left_paddle1: 'L B1',
  left_paddle2: 'L B2',
  right_paddle1: 'R B1',
  right_paddle2: 'R B2',
}

function ShellArtwork() {
  return (
    <g aria-hidden="true">
      <path className={styles.shell} d={GENERIC_PATHS.shell} />
      <path className={styles.shellLine} d={GENERIC_PATHS.leftGrip} />
      <path className={styles.shellLine} d={GENERIC_PATHS.rightGrip} />
      <circle className={styles.detailLine} cx={424} cy={456} r={77} />
      <rect className={styles.detailLine} x={402} y={435} width={44} height={42} rx={3} />
    </g>
  )
}

function ButtonBubble({
  cx,
  cy,
  label,
  pressed = false,
  radius = 23,
  bound = false,
  selected = false,
  onSelect,
  title,
}: ButtonBubbleProps) {
  return (
    <g className={onSelect ? styles.interactive : undefined} onClick={onSelect}>
      {title && <title>{title}</title>}
      <circle
        className={join(
          styles.control,
          bound && styles.controlBound,
          selected && styles.controlSelected,
          pressed && styles.controlPressed
        )}
        cx={cx}
        cy={cy}
        r={radius}
      />
      <text
        className={join(
          styles.controlText,
          selected && styles.controlTextSelected,
          pressed && styles.controlTextPressed
        )}
        x={cx}
        y={cy}
      >
        {label}
      </text>
    </g>
  )
}

function PathButton({
  d,
  label,
  labelX,
  labelY,
  pressed = false,
  compact = false,
  bound = false,
  selected = false,
  onSelect,
  title,
}: PathButtonProps) {
  return (
    <g className={onSelect ? styles.interactive : undefined} onClick={onSelect}>
      {title && <title>{title}</title>}
      <path
        className={join(
          styles.control,
          compact && styles.controlCompact,
          bound && styles.controlBound,
          selected && styles.controlSelected,
          pressed && styles.controlPressed
        )}
        d={d}
      />
      <text
        className={join(
          styles.controlText,
          compact && styles.controlTextCompact,
          selected && styles.controlTextSelected,
          pressed && styles.controlTextPressed
        )}
        x={labelX}
        y={labelY}
      >
        {label === '▱' ? '' : label}
      </text>
      {label === '▱' && (
        <g className={join(styles.buttonIcon, (selected || pressed) && styles.buttonIconActive)} aria-hidden="true">
          <rect x={labelX - 12} y={labelY - 9} width={19} height={14} rx={1} />
          <rect x={labelX - 5} y={labelY - 3} width={19} height={14} rx={1} />
        </g>
      )}
    </g>
  )
}

function PaddleButton({
  height,
  label,
  pressed = false,
  width,
  x,
  y,
  bound = false,
  selected = false,
  onSelect,
  title,
}: PaddleButtonProps) {
  return (
    <g className={onSelect ? styles.interactive : undefined} onClick={onSelect}>
      {title && <title>{title}</title>}
      <rect
        className={join(
          styles.control,
          styles.paddleControl,
          bound && styles.controlBound,
          selected && styles.controlSelected,
          pressed && styles.controlPressed
        )}
        x={x}
        y={y}
        width={width}
        height={height}
        rx={height / 2}
      />
      <text
        className={join(
          styles.controlText,
          styles.paddleText,
          selected && styles.controlTextSelected,
          pressed && styles.controlTextPressed
        )}
        x={x + width / 2}
        y={y + height / 2}
      >
        {label}
      </text>
    </g>
  )
}

function TriggerPath({
  d,
  fillX,
  fillY,
  fillWidth,
  label,
  labelX,
  labelY,
  value,
  pressed = false,
  bound = false,
  selected = false,
  onSelect,
  title,
}: TriggerPathProps) {
  const fillValue = clamp(value, 0, 1)

  return (
    <g className={onSelect ? styles.interactive : undefined} onClick={onSelect}>
      {title && <title>{title}</title>}
      <path
        className={join(
          styles.triggerTrack,
          bound && styles.triggerTrackBound,
          selected && styles.triggerTrackSelected,
          pressed && styles.controlPressed
        )}
        d={d}
      />
      <rect className={styles.triggerFill} x={fillX} y={fillY} width={fillWidth * fillValue} height={12} rx={6} />
      <text className={join(styles.triggerLabel, selected && styles.triggerLabelSelected)} x={labelX} y={labelY}>
        {label}
      </text>
    </g>
  )
}

function Stick({
  cx,
  cy,
  x,
  y,
  pressed = false,
  bound = false,
  selected = false,
  onSelect,
  title,
}: StickProps) {
  const knobX = cx + clamp(x, -1, 1) * 27
  const knobY = cy + 16 + clamp(-y, -1, 1) * 27

  return (
    <g className={onSelect ? styles.interactive : undefined} onClick={onSelect}>
      {title && <title>{title}</title>}
      <circle
        className={join(styles.stickBase, bound && styles.stickBaseBound, selected && styles.stickBaseSelected)}
        cx={cx}
        cy={cy}
        r={76}
      />
      <circle className={join(styles.stickRing, bound && styles.stickRingBound)} cx={cx} cy={cy} r={64} />
      <circle className={styles.stickRing} cx={knobX} cy={knobY - 8} r={55} />
      <circle
        className={join(
          styles.stickKnob,
          selected && styles.stickKnobSelected,
          pressed && styles.stickKnobPressed
        )}
        cx={knobX}
        cy={knobY}
        r={40}
      />
    </g>
  )
}

export default function Controller({
  bindingCounts,
  device,
  status,
  selectedControl,
  onSelectControl,
}: ControllerStatusSvgProps) {
  // Live button state uses the runtime input IDs. Mapping counts group the
  // different inputs produced by a physical trigger, stick or touchpad.
  const pressed = new Set<(typeof buttonNames)[number] | 'left_trigger' | 'right_trigger'>(buttonNames.filter((_, index) => status?.buttons[index]));
  const [leftStickX, leftStickY, rightStickX, rightStickY, leftTrigger, rightTrigger] = status?.axes ?? [0, 0, 0, 0, 0, 0];
  if (leftTrigger > 0.2) pressed.add('left_trigger');
  if (rightTrigger > 0.2) pressed.add('right_trigger');
  const mapping = (control: string): SharedControlProps => ({
    bound: bindingCounts?.has(control),
    selected: control === selectedControl,
    onSelect: bindingCounts?.has(control) && onSelectControl ? () => onSelectControl(control) : undefined,
  })
  const visiblePaddles = PADDLE_LAYOUT.filter(({command}) =>
    device?.buttons.includes(command) || pressed.has(command) || bindingCounts?.has(command) || selectedControl === command
  )

  return (
    <div className={styles.visualizer}>
      <svg
        className={styles.controllerSvg}
        viewBox={visiblePaddles.length ? "0 0 1117 780" : "0 0 1117 700"}
        role="img"
        aria-label="通用手柄预览"
      >
        <ShellArtwork />

        <TriggerPath
          d={GENERIC_PATHS.leftTrigger}
          fillX={245}
          fillY={55}
          fillWidth={42}
          label="LT"
          labelX={270}
          labelY={46}
          value={leftTrigger}
          pressed={pressed.has('left_trigger')}
          {...mapping('left_trigger')}
          title="Left trigger"
        />
        <PathButton
          d={GENERIC_PATHS.leftBumper}
          label="LB"
          compact
          labelX={270}
          labelY={107}
          pressed={pressed.has('left_shoulder')}
          {...mapping('left_shoulder')}
          title="Left bumper"
        />

        <TriggerPath
          d={GENERIC_PATHS.rightTrigger}
          fillX={837}
          fillY={55}
          fillWidth={42}
          label="RT"
          labelX={860}
          labelY={46}
          value={rightTrigger}
          pressed={pressed.has('right_trigger')}
          {...mapping('right_trigger')}
          title="Right trigger"
        />
        <PathButton
          d={GENERIC_PATHS.rightBumper}
          label="RB"
          compact
          labelX={860}
          labelY={107}
          pressed={pressed.has('right_shoulder')}
          {...mapping('right_shoulder')}
          title="Right bumper"
        />

        <g>
          <PathButton
            d={GENERIC_PATHS.dpadUp}
            label=""
            labelX={410}
            labelY={450}
            pressed={pressed.has('dpad_up')}
            {...mapping('dpad_up')}
            title="D-pad up"
          />
          <PathButton
            d={GENERIC_PATHS.dpadLeft}
            label=""
            labelX={355}
            labelY={505}
            pressed={pressed.has('dpad_left')}
            {...mapping('dpad_left')}
            title="D-pad left"
          />
          <PathButton
            d={GENERIC_PATHS.dpadRight}
            label=""
            labelX={465}
            labelY={505}
            pressed={pressed.has('dpad_right')}
            {...mapping('dpad_right')}
            title="D-pad right"
          />
          <PathButton
            d={GENERIC_PATHS.dpadDown}
            label=""
            labelX={410}
            labelY={560}
            pressed={pressed.has('dpad_down')}
            {...mapping('dpad_down')}
            title="D-pad down"
          />
          <PathButton
            d={GENERIC_PATHS.view}
            label="▱"
            labelX={482}
            labelY={300}
            pressed={pressed.has('back')}
            compact
            {...mapping('back')}
            title="View / Back button"
          />
        </g>

        <PathButton
          d={GENERIC_PATHS.share}
          compact
          label="↑"
          labelX={562}
          labelY={354}
          pressed={pressed.has('misc1')}
          {...mapping('misc1')}
          title="Share / Capture button"
        />

        <g>
          <ButtonBubble
            cx={830}
            cy={228}
            radius={34.957}
            label="Y"
            pressed={pressed.has('north')}
            {...mapping('north')}
            title="North face button"
          />
          <ButtonBubble
            cx={900}
            cy={298}
            radius={34.957}
            label="B"
            pressed={pressed.has('east')}
            {...mapping('east')}
            title="East face button"
          />
          <ButtonBubble
            cx={830}
            cy={360}
            radius={34.957}
            label="A"
            pressed={pressed.has('south')}
            {...mapping('south')}
            title="South face button"
          />
          <ButtonBubble
            cx={760}
            cy={298}
            radius={34.957}
            label="X"
            pressed={pressed.has('west')}
            {...mapping('west')}
            title="West face button"
          />
          <PathButton
            d={GENERIC_PATHS.menu}
            label="≡"
            labelX={642}
            labelY={300}
            pressed={pressed.has('start')}
            compact
            {...mapping('start')}
            title="Menu / Start button"
          />
        </g>

        <Stick
          cx={292}
          cy={298}
          x={leftStickX}
          y={-leftStickY}
          pressed={pressed.has('left_stick_click')}
          {...mapping('left_stick')}
          title="Left stick"
        />
        <Stick
          cx={698}
          cy={450}
          x={rightStickX}
          y={-rightStickY}
          pressed={pressed.has('right_stick_click')}
          {...mapping('right_stick')}
          title="Right stick"
        />

        {visiblePaddles.map(entry => {
          return (
            <PaddleButton
              key={entry.command}
              x={entry.x}
              y={entry.y}
              width={entry.width}
              height={entry.height}
              label={paddleLabels[entry.command]}
              pressed={pressed.has(entry.command)}
              {...mapping(entry.command)}
              title={BACK_INPUT_TITLES[entry.command]}
            />
          )
        })}

        <ButtonBubble
          cx={562}
          cy={200}
          radius={24}
          label=""
          pressed={pressed.has('guide')}
          {...mapping('guide')}
          title="Home / Guide button"
        />
        {(device?.buttons.includes('misc2') || pressed.has('misc2') || bindingCounts?.has('misc2') || selectedControl === 'misc2') && (
          <PathButton
            d={GENERIC_PATHS.mic}
            label="M2"
            labelX={562}
            labelY={421}
            pressed={pressed.has('misc2')}
            compact
            {...mapping('misc2')}
            title="扩展键 2"
          />
        )}
        {(bindingCounts?.has('touchpad') || selectedControl === 'touchpad') && (
          <PathButton
            d={GENERIC_PATHS.touchpad}
            label="TP"
            labelX={562}
            labelY={251}
            pressed={pressed.has('touchpad')}
            compact
            {...mapping('touchpad')}
            title="触摸板"
          />
        )}
        {bindingCounts && <g aria-hidden="true">{BINDING_BADGES.map(([control, x, y]) => {
          const count = bindingCounts.get(control)
          if (!count) return null
          const width = Math.max(49, String(count).length * 21 + 20)
          return <g key={control} data-control={control} data-selected={selectedControl === control}
            className={join(styles.bindingCount, onSelectControl && styles.bindingCountInteractive)}
            onClick={onSelectControl ? () => onSelectControl(control) : undefined}>
            <rect x={x - width / 2} y={y - 24} width={width} height={48} rx={23}/>
            <text x={x} y={y}>{count}</text>
          </g>
        })}</g>}
      </svg>
    </div>
  )
}
