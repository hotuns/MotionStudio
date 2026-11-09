import {t,type Language} from '../i18n';
import { Fragment, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { version } from '../../package.json';
import { StudioIcon, type StudioIconName } from './StudioIcon';
import {OptionPicker} from './OptionPicker';
import {gameTabs, type GameTab} from './game-tabs';
import styles from './StudioShell.module.css';
import themeStyles from './ThemeToggle.module.css';

const pages = {
  overview: { label: '工作台', description: '连接设备、选择配置，然后开始调校。', icon: 'overview' },
  games: { label: '游戏配置', description: '在同一份配置里设置动作、摇杆、扳机、触控和体感。', icon: 'games' },
  controllerStatus: { label: '设备与输入', description: '检查连接、按键与传感器的实时输入。', icon: 'controllerStatus' },
  profiles: { label: '配置库', description: '为不同游戏管理、导入和恢复配置。', icon: 'profiles' },
  switching: { label: '自动切换', description: '为游戏菜单、战斗和快捷键指定要使用的配置。', icon: 'refresh' },
  settings: { label: '设置与组件', description: '检查驱动、管理手柄输出和查看引擎状态。', icon: 'settings' },
} as const;

export type StudioPage = keyof typeof pages;
const navigation: { label?: string; pages: StudioPage[] }[] = [
  { pages: ['overview', 'games', 'controllerStatus'] },
  { label: '资源', pages: ['profiles', 'switching', 'settings'] },
];

type Command = { label: string; icon: StudioIconName; run: () => void };
type Props = {
  page: StudioPage;
  gameTab: GameTab;
  onNavigate: (page: StudioPage) => void;
  onGameTab: (tab: GameTab) => void;
  children: ReactNode;
  profileId: string;
  profiles: { id: string; name: string }[];
  onSelectProfile: (id: string) => void;
  layerControls?: ReactNode;
  onNewProfile: () => void;
  onImportJsm: () => void;
  onGamepad: () => void;
  gamepadOn: boolean;
  gamepadAvailable: boolean;
  onOpenFolder: () => void;
  onHelp: () => void;
  outputOn: boolean;
  waiting: boolean;
  preparing?: boolean;
  onStartMapping: () => void;
  onStopMapping: () => void;
  calibrating: boolean;
  outputDisabled: boolean;
  busy: boolean;
  dirty: boolean;
  pending: boolean;
  canApply: boolean;
  deviceCount: number;
  status: string;
  onSave: () => void;
  onApply: () => void;
  onDiscard: () => void;
  capturing: boolean;
  language:Language;
  languageDisabled:boolean;
  onLanguage:(language:Language)=>void;
};

export function StudioShell(props: Props) {
  const [theme, setTheme] = useState<'light' | 'dark'>(() => localStorage.getItem('motion-studio-theme') === 'dark' ? 'dark' : 'light');
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState('');
  const palette = useRef<HTMLDialogElement>(null);
  const scrollArea = useRef<HTMLDivElement>(null);
  const latest = useRef(props);
  latest.current = props;

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('motion-studio-theme', theme);
  }, [theme]);
  useEffect(() => { scrollArea.current?.scrollTo(0, 0); }, [props.page, props.gameTab]);
  useEffect(() => { if (searchOpen && !palette.current?.open) palette.current?.showModal(); }, [searchOpen]);
  const openSearch = () => { setQuery(''); setSearchOpen(true); };
  useEffect(() => {
    const keyDown = (event: KeyboardEvent) => {
      if (latest.current.capturing || event.defaultPrevented || event.repeat || event.altKey ||
          !(event.ctrlKey || event.metaKey) || document.querySelector('dialog[open]')) return;
      if (event.key.toLowerCase() === 'k') {
        event.preventDefault();
        openSearch();
      } else if (event.key.toLowerCase() === 's') {
        event.preventDefault();
        latest.current.onSave();
      } else if (event.key === 'Enter' && latest.current.canApply) {
        event.preventDefault();
        latest.current.onApply();
      }
    };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, []);

  const tools: Command[] = [
    { label: '打开配置文件夹', icon: 'folder', run: props.onOpenFolder },
    { label: '自动切换', icon: 'profiles', run: () => props.onNavigate('switching') },
    { label: '帮助与快捷键', icon: 'help', run: props.onHelp },
  ];
  const allCommands: Command[] = [
    ...navigation.flatMap(group => group.pages).map(page => ({ ...pages[page], run: () => props.onNavigate(page) })),
    ...gameTabs.map(tab => ({ label: tab.shortcut, icon: tab.icon, run: () => props.onGameTab(tab.id) })),
    { label: '新建配置', icon: 'profiles', run: props.onNewProfile },
    { label: '导入 JSM 配置', icon: 'profiles', run: props.onImportJsm },
    ...tools.filter(command => command.label !== '自动切换'),
  ];
  const commands = allCommands.filter(command => (t(command.label)+(command.label===pages.games.label?' '+t('按键映射 组合键 鼠标滚轮 侧键 摇杆设置'):'')).toLowerCase().includes(query.trim().toLowerCase()));
  const runCommand = (command: Command) => { palette.current?.close(); command.run(); };
  const navButton = (page: StudioPage) => (
    <button type="button" key={page} className={styles.navItem} aria-current={props.page === page ? 'page' : undefined}
      aria-label={pages[page].label} title={pages[page].label} onClick={() => props.onNavigate(page)}>
      <StudioIcon name={pages[page].icon} /><span>{pages[page].label}</span>
    </button>
  );

  const heading = pages[props.page];
  const blocked = props.calibrating || props.busy;
  const outputBlocked = blocked || props.capturing;
  const light = theme === 'light';
  const outputLabel = props.preparing ? '正在准备输出' : props.waiting ? '等待手柄连接' : props.outputOn ? '映射运行中'
    : props.calibrating || props.capturing ? '输出已暂停' : '映射已停止';
  const stopAction = props.outputOn || props.waiting;
  const actionLabel = props.preparing ? '取消启动' : props.waiting ? '取消等待' : stopAction ? '停止映射' : '启动映射';

  return (
    <div className={styles.shell}>
      <aside className={styles.sidebar} data-pad-zone="sidebar">
        <div className={styles.brand}>
          <span className={styles.brandMark} aria-hidden="true"><i /><i /><i /></span>
          <span><strong>Motion Studio</strong><small>CONTROL WORKSPACE</small></span>
        </div>
        <button type="button" className={styles.searchButton} aria-label="搜索页面或操作" onClick={openSearch}>
          <StudioIcon name="search" size={16} /><span>搜索页面或操作…</span><kbd>Ctrl K</kbd>
        </button>
        <nav aria-label="控制工作区" className={styles.navigation}>
          {navigation.map(group => (
            <Fragment key={group.pages[0]}>
              {group.label && <div className={styles.navLabel}>{group.label}</div>}
              <div className={styles.navGroup}>{group.pages.map(navButton)}</div>
            </Fragment>
          ))}
        </nav>
        <div className={styles.sidebarBottom}>
          <button type="button" className={styles.community} onClick={props.onHelp}>
            <StudioIcon name="help" size={16} />支持与帮助<span>↗</span>
          </button>
          <div className={styles.preferences}>
            <label title="界面语言">
              <span className="language-select-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18 M12 3c5 5 5 13 0 18 M12 3c-5 5-5 13 0 18"/></svg></span>
              <select aria-label="界面语言" value={props.language} disabled={props.languageDisabled} onChange={event=>props.onLanguage(event.target.value as Language)}>
                <option value="zh-CN" translate="no">简体中文</option><option value="en" translate="no">English</option>
              </select>
            </label>
            <button type="button" className={[themeStyles.themeToggle, themeStyles.compact, light ? themeStyles.on : ''].join(' ')}
              aria-label={light ? '切换为深色主题' : '切换为浅色主题'} aria-pressed={light}
              title={light ? '切换为深色主题' : '切换为浅色主题'}
              onClick={() => setTheme(light ? 'dark' : 'light')}>
              <span className={themeStyles.labelGroup}><span className={themeStyles.icon} aria-hidden="true">{light ? '☀' : '☾'}</span></span>
              <span className={themeStyles.switch} aria-hidden="true"><span className={themeStyles.thumb} /></span>
            </button>
          </div>
          <div className={styles.version}>Motion Studio <span>v{version}</span></div>
        </div>
      </aside>
      <div className={styles.workspace}>
        <header className={styles.toolbar} data-pad-zone="toolbar">
          <div className={styles.profileTools}>
            <div className={styles.profileSelect} data-profile-picker>
              <StudioIcon name="profiles" size={18} /><span>当前配置</span>
              <OptionPicker label="当前配置" placeholder="选择配置" value={props.profileId || null} disabled={blocked || !props.profiles.length}
                groups={[{options:props.profiles.map(profile => ({value:profile.id, label:profile.name, localize:false}))}]}
                onChange={props.onSelectProfile}/>
              {props.dirty && <span className={styles.dirtyDot} title="有未保存的修改" />}
            </div>
            {props.layerControls}
            <button type="button" className="secondary-btn" disabled={blocked} onClick={props.onNewProfile}>新建配置</button>
          </div>
          <div className={styles.toolbarActions}>
            <button type="button" className="secondary-btn" aria-label="手柄操作界面" aria-pressed={props.gamepadOn} disabled={outputBlocked||!props.gamepadAvailable} onClick={props.onGamepad} title="方向移动，A 确认，B 返回；View + Menu 切回界面">
              <StudioIcon name="games" size={16}/>{props.gamepadOn?'手柄操作中':'手柄操作'}
            </button>
            <div className={styles.outputControls}>
              <span className={styles.outputStatus} data-running={props.outputOn} role="status">
                <span className={styles.statusDot} aria-hidden="true" />{outputLabel}
              </span>
              <button type="button" className={styles.mappingAction + ' ' + (stopAction ? 'danger-btn' : 'primary-btn')}
                disabled={outputBlocked || props.outputDisabled} onClick={stopAction ? props.onStopMapping : props.onStartMapping}>
                <StudioIcon name={stopAction ? 'stop' : 'play'} size={16}/>{actionLabel}
              </button>
            </div>
            <details className={styles.tools}>
              <summary aria-label="更多工具" title="更多工具"><StudioIcon name="more" /></summary>
              <div className={styles.toolsMenu}>{tools.map(command => (
                <button key={command.label} type="button" onClick={event => {
                  event.currentTarget.closest('details')?.removeAttribute('open');
                  command.run();
                }}><StudioIcon name={command.icon} size={16} />{command.label}</button>
              ))}</div>
            </details>
          </div>
        </header>
        <div className={styles.scrollArea} ref={scrollArea} data-pad-scroll>
          <main className={styles.main} id="studio-workspace" data-pad-zone="content">
            <div className={styles.pageHeading}>
              <div>
                <div className={styles.breadcrumb}>Motion Studio <span>/</span> {heading.label}</div>
                <h1>{heading.label}</h1>
                <p>{heading.description}</p>
              </div>
            </div>
            {props.children}
          </main>
        </div>
        <footer className={[styles.footer, props.pending ? styles.footerPending : ''].join(' ')} data-pad-zone="footer">
          <div className={styles.editState} role="status">
            <span className={props.pending ? styles.dirtyDot : styles.savedIcon}>
              {!props.pending && <StudioIcon name="check" size={15} />}
            </span>
            <span>
              <strong>{props.dirty ? '有未保存的修改' : props.pending ? '当前配置尚未应用' : '没有待应用的修改'}</strong>
              <small title={props.status}>{props.status}</small>
            </span>
          </div>
          <div className={styles.footerActions}>
            <span className={styles.connection}><span className={styles.statusDot} />{props.deviceCount ? props.deviceCount + ' 台设备' : '未连接设备'}</span>
            {props.dirty && <button type="button" className="ghost-btn" disabled={blocked} onClick={props.onDiscard}>撤销修改</button>}
            <button type="button" className="secondary-btn" disabled={!props.dirty || outputBlocked} onClick={props.onSave}>保存配置<kbd>Ctrl S</kbd></button>
            <button type="button" className="primary-btn" disabled={!props.canApply || outputBlocked} onClick={props.onApply}>应用修改<kbd>Ctrl ↵</kbd></button>
          </div>
        </footer>
      </div>
      {searchOpen && <dialog className={styles.palette} ref={palette} aria-labelledby="studio-search-title" onClose={() => setSearchOpen(false)}>
        <h2 className={styles.srOnly} id="studio-search-title">搜索页面或操作</h2>
        <div className={styles.paletteHeader}>
          <StudioIcon name="search" />
          <input autoFocus aria-label="搜索页面或操作" placeholder="搜索页面或操作…" value={query} onChange={event => setQuery(event.target.value)}
            onKeyDown={event => {
              if (event.key === 'Enter' && commands[0]) runCommand(commands[0]);
              if (event.key === 'ArrowDown') { event.preventDefault(); palette.current?.querySelector<HTMLButtonElement>('[data-command]')?.focus(); }
            }} />
          <button type="button" className={styles.closeButton} aria-label="关闭搜索" onClick={() => palette.current?.close()}><StudioIcon name="close" size={18} /></button>
        </div>
        <p className={styles.paletteHint}>输入功能名称，快速前往</p>
        <div className={styles.commandList}>
          {commands.map(command => <button type="button" data-command key={command.label} onClick={() => runCommand(command)}>
            <StudioIcon name={command.icon} size={18} /><span>{command.label}</span><StudioIcon name="arrow" size={16} />
          </button>)}
          {!commands.length && <p>没有匹配的操作</p>}
        </div>
      </dialog>}
    </div>
  );
}
