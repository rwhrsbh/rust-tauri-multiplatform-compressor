import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { AnimatePresence, motion } from "motion/react";
import {
  AlertTriangle,
  ArchiveRestore,
  ArrowLeft,
  Check,
  ChevronsDownUp,
  FolderOpen,
  FolderSearch,
  Gamepad2,
  HardDrive,
  History,
  Languages,
  Loader2,
  Pause,
  Play,
  RefreshCw,
  ShieldCheck,
  Square,
  Trash2,
  X,
} from "lucide-react";
import {
  detectLang,
  LANGS,
  makeT,
  numberLocale,
  saveLang,
  type Lang,
} from "./i18n";

/* ---------- Типы, зеркалящие структуры Rust ---------- */

interface DiskInfo {
  path: string;
  mount_point: string;
  filesystem: string;
  total_bytes: number;
  free_bytes: number;
  supported: boolean;
  reason: string;
  reason_code: string;
}

interface AnalysisSummary {
  root: string;
  total_files: number;
  compressible_files: number;
  skipped_files: number;
  total_bytes: number;
  estimated_bytes: number;
  estimated_savings_ratio: number;
  already_compressed_files: number;
  already_saved_bytes: number;
  proton_hint: boolean;
  estimated_bytes_by_algo: Record<string, number>;
}

interface GameEntry {
  name: string;
  path: string;
  launcher: string;
  cover: string | null;
}

type Algo =
  | "xpress4k"
  | "xpress8k"
  | "xpress16k"
  | "lzx"
  | "zstd"
  | "zlib"
  | "lzo";

/* WOF — Windows/NTFS; Btrfs — Linux. macOS (decmpfs) — всегда zlib, без выбора */
const WOF_ALGOS: Algo[] = ["xpress4k", "xpress8k", "xpress16k", "lzx"];
const BTRFS_ALGOS: Algo[] = ["zstd", "zlib", "lzo"];
const ALGO_IDS: Algo[] = [...WOF_ALGOS, ...BTRFS_ALGOS];
const ALGO_LABELS: Record<Algo, string> = {
  xpress4k: "X4K",
  xpress8k: "X8K",
  xpress16k: "X16K",
  lzx: "LZX",
  zstd: "ZSTD",
  zlib: "ZLIB",
  lzo: "LZO",
};
const ALGO_KEY = "gc-algo";

interface HistoryEntry {
  root: string;
  date: number;
  files: number;
  original_bytes: number;
  saved_bytes: number;
  partial: boolean;
  algorithm?: Algo | null;
}

interface StalenessPayload {
  root: string;
  status: "ok" | "stale" | "missing";
  uncompressed_files: number;
  potential_saved_bytes: number;
}

interface ProgressPayload {
  mode: "compress" | "decompress";
  state: "running" | "paused" | "cancelling";
  processed_files: number;
  total_files: number;
  percent: number;
  bytes_processed: number;
  total_bytes: number;
  saved_bytes: number;
  speed_bps: number;
  current_file: string;
  elapsed_secs: number;
}

interface DonePayload {
  mode: "compress" | "decompress";
  cancelled: boolean;
  processed_files: number;
  total_files: number;
  failed_files: number;
  not_beneficial_files: number;
  original_bytes: number;
  final_physical_bytes: number;
  saved_bytes: number;
  elapsed_secs: number;
  errors: string[];
}

type Screen = "dashboard" | "working" | "done";
type DashTab = "compress" | "library" | "history";
const DASH_TABS: DashTab[] = ["compress", "library", "history"];

/* ---------- Анимационные пресеты ---------- */

const EASE = [0.22, 1, 0.36, 1] as const;

const screenVariants = {
  initial: { opacity: 0, y: 18, scale: 0.985 },
  animate: {
    opacity: 1,
    y: 0,
    scale: 1,
    transition: { duration: 0.4, ease: EASE, staggerChildren: 0.06 },
  },
  exit: {
    opacity: 0,
    y: -14,
    scale: 0.99,
    transition: { duration: 0.22, ease: "easeIn" as const },
  },
};

const itemVariants = {
  initial: { opacity: 0, y: 14 },
  animate: { opacity: 1, y: 0, transition: { duration: 0.45, ease: EASE } },
};

/* ---------- Хук: плавная интерполяция числа ---------- */

function useSmoothNumber(target: number, rate = 0.16): number {
  const [value, setValue] = useState(target);
  const raf = useRef(0);
  useEffect(() => {
    cancelAnimationFrame(raf.current);
    const step = () => {
      setValue((v) => {
        const next = v + (target - v) * rate;
        if (Math.abs(next - target) < 0.02) return target;
        raf.current = requestAnimationFrame(step);
        return next;
      });
    };
    raf.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf.current);
  }, [target, rate]);
  return value;
}

/* ---------- Мелкие компоненты ---------- */

function LogoMark() {
  return (
    <svg width="30" height="30" viewBox="0 0 30 30" fill="none" aria-hidden>
      <defs>
        <linearGradient id="lg" x1="0" y1="0" x2="30" y2="30">
          <stop offset="0" stopColor="#7c7ff2" />
          <stop offset="1" stopColor="#38cfd9" />
        </linearGradient>
      </defs>
      <rect
        x="0.75"
        y="0.75"
        width="28.5"
        height="28.5"
        rx="8"
        stroke="url(#lg)"
        strokeOpacity="0.55"
        strokeWidth="1.5"
      />
      <path
        d="M9.5 8.5 15 13.5l5.5-5M9.5 21.5 15 16.5l5.5 5"
        stroke="url(#lg)"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function Stat({
  label,
  value,
  tone = "default",
}: {
  label: string;
  value: string;
  tone?: "default" | "accent" | "success";
}) {
  const color = {
    default: "text-[--text-1]",
    accent: "gradient-text",
    success: "text-[--success]",
  }[tone];
  return (
    <div className="card card-hover px-5 py-4">
      <div className="label">{label}</div>
      <div
        className={`display-num mt-1.5 text-[19px] font-semibold ${color}`}
      >
        {value}
      </div>
    </div>
  );
}

function Btn({
  children,
  onClick,
  disabled,
  kind = "default",
  className = "",
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  kind?: "default" | "primary" | "danger";
  className?: string;
}) {
  return (
    <motion.button
      whileTap={{ scale: 0.97 }}
      onClick={onClick}
      disabled={disabled}
      className={`btn ${kind === "primary" ? "btn-primary" : ""} ${kind === "danger" ? "btn-danger" : ""} ${className}`}
    >
      {children}
    </motion.button>
  );
}

/* Кольцевой прогресс с градиентным штрихом */
function Ring({ percent, paused }: { percent: number; paused: boolean }) {
  const smooth = useSmoothNumber(percent);
  const R = 88;
  const C = 2 * Math.PI * R;
  return (
    <div className="relative grid place-items-center">
      <svg width="220" height="220" viewBox="0 0 220 220">
        <defs>
          <linearGradient id="ring" x1="0" y1="0" x2="220" y2="220">
            <stop offset="0" stopColor="#7c7ff2" />
            <stop offset="1" stopColor="#38cfd9" />
          </linearGradient>
        </defs>
        <circle
          cx="110"
          cy="110"
          r={R}
          fill="none"
          stroke="rgba(255,255,255,0.06)"
          strokeWidth="10"
        />
        <motion.circle
          cx="110"
          cy="110"
          r={R}
          fill="none"
          stroke="url(#ring)"
          strokeWidth="10"
          strokeLinecap="round"
          strokeDasharray={C}
          animate={{ strokeDashoffset: C * (1 - smooth / 100) }}
          transition={{ duration: 0.2, ease: "linear" }}
          transform="rotate(-90 110 110)"
          style={{
            filter: paused
              ? "none"
              : "drop-shadow(0 0 14px rgba(124,127,242,0.45))",
          }}
        />
      </svg>
      <div className="absolute text-center">
        <div className="display-num text-[44px] font-bold leading-none text-[--text-1]">
          {smooth.toFixed(1)}
          <span className="text-2xl text-[--text-3]">%</span>
        </div>
      </div>
    </div>
  );
}

/* ---------- Главный компонент ---------- */

export default function App() {
  const [lang, setLang] = useState<Lang>(() => detectLang());
  const t = useMemo(() => makeT(lang), [lang]);
  const nfmt = useMemo(() => new Intl.NumberFormat(numberLocale(lang)), [lang]);

  const [screen, setScreen] = useState<Screen>("dashboard");
  const [langOpen, setLangOpen] = useState(false);
  const [gamePath, setGamePath] = useState<string | null>(null);
  const [disk, setDisk] = useState<DiskInfo | null>(null);
  const [analysis, setAnalysis] = useState<AnalysisSummary | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [scannedFiles, setScannedFiles] = useState(0);
  const [progress, setProgress] = useState<ProgressPayload | null>(null);
  const [done, setDone] = useState<DonePayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fileLog, setFileLog] = useState<{ id: number; path: string }[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [staleness, setStaleness] = useState<Record<string, StalenessPayload>>(
    {},
  );
  const [checkingStale, setCheckingStale] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [library, setLibrary] = useState<GameEntry[]>([]);
  const [dashTab, setDashTab] = useState<DashTab>("compress");
  const [algo, setAlgo] = useState<Algo>(() => {
    const s = localStorage.getItem(ALGO_KEY) as Algo | null;
    return s && ALGO_IDS.includes(s) ? s : "xpress8k";
  });
  const logRef = useRef<string>("");
  const logId = useRef(0);
  // Рефы для слушателей, зарегистрированных один раз при монтировании
  const screenRef = useRef<Screen>("dashboard");
  const analyzingRef = useRef(false);
  const tRef = useRef(t);
  screenRef.current = screen;
  analyzingRef.current = analyzing;
  tRef.current = t;

  const refreshHistory = useCallback(async () => {
    try {
      setHistory(await invoke<HistoryEntry[]>("get_history"));
    } catch {
      /* история недоступна — не критично */
    }
  }, []);

  const fmtBytes = useCallback(
    (n: number): string => {
      if (!Number.isFinite(n)) return "—";
      const neg = n < 0;
      let v = Math.abs(n);
      const units =
        lang === "ru" || lang === "uk"
          ? ["Б", "КБ", "МБ", "ГБ", "ТБ"]
          : ["B", "KB", "MB", "GB", "TB"];
      let i = 0;
      while (v >= 1024 && i < units.length - 1) {
        v /= 1024;
        i++;
      }
      return `${neg ? "-" : ""}${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
    },
    [lang],
  );

  const fmtInt = useCallback((n: number) => nfmt.format(n), [nfmt]);

  const changeLang = useCallback((l: Lang) => {
    setLang(l);
    saveLang(l);
  }, []);

  const changeAlgo = useCallback((a: Algo) => {
    setAlgo(a);
    localStorage.setItem(ALGO_KEY, a);
  }, []);

  /* Доступные алгоритмы зависят от ФС выбранного диска */
  const isNtfs = disk?.filesystem.toUpperCase() === "NTFS";
  const isBtrfs = disk?.filesystem.toLowerCase().includes("btrfs") ?? false;
  const algoChoices = isNtfs ? WOF_ALGOS : isBtrfs ? BTRFS_ALGOS : [];
  /* Сохранённый выбор может быть с другой платформы — берём первый доступный */
  const effAlgo: Algo = algoChoices.includes(algo)
    ? algo
    : (algoChoices[0] ?? algo);

  /* Подписка на события бэкенда */
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    (async () => {
      unlisteners.push(
        await listen<ProgressPayload>("compression://progress", (e) => {
          setProgress(e.payload);
          const f = e.payload.current_file;
          if (f && f !== logRef.current) {
            logRef.current = f;
            setFileLog((prev) =>
              [{ id: logId.current++, path: f }, ...prev].slice(0, 7),
            );
          }
        }),
        await listen<DonePayload>("compression://done", (e) => {
          setDone(e.payload);
          setScreen("done");
          void refreshHistory();
        }),
        await listen<{ scanned_files: number }>(
          "compression://scan-progress",
          (e) => setScannedFiles(e.payload.scanned_files),
        ),
        await listen<StalenessPayload>("history://staleness", (e) =>
          setStaleness((prev) => ({ ...prev, [e.payload.root]: e.payload })),
        ),
        await listen("history://staleness-done", () =>
          setCheckingStale(false),
        ),
      );
    })();
    void refreshHistory();
    // Фоновая проверка: не устарело ли сжатие папок из истории
    setCheckingStale(true);
    invoke("check_history_staleness").catch(() => setCheckingStale(false));
    // Библиотека игр из лаунчеров
    invoke<GameEntry[]>("get_game_library")
      .then(setLibrary)
      .catch(() => {});
    return () => unlisteners.forEach((u) => u());
  }, [refreshHistory]);

  const mainRef = useRef<HTMLElement>(null);

  /* Проверка ФС + анализ выбранного пути (диалог, drag & drop, библиотека) */
  const analyzePath = useCallback(async (selected: string) => {
    setError(null);
    setGamePath(selected);
    setDashTab("compress");
    mainRef.current?.scrollTo({ top: 0, behavior: "smooth" });
    setDisk(null);
    setAnalysis(null);
    setDone(null);
    setProgress(null);
    setFileLog([]);

    try {
      const info = await invoke<DiskInfo>("check_filesystem", {
        path: selected,
      });
      setDisk(info);
      if (info.supported) {
        setAnalyzing(true);
        setScannedFiles(0);
        const summary = await invoke<AnalysisSummary>("analyze_folder", {
          path: selected,
        });
        setAnalysis(summary);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setAnalyzing(false);
    }
  }, []);

  const analyzePathRef = useRef(analyzePath);
  analyzePathRef.current = analyzePath;

  /* Выбор папки через системный диалог */
  const pickFolder = useCallback(async () => {
    setError(null);
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("dialog.pickTitle"),
    });
    if (typeof selected !== "string") return;
    await analyzePath(selected);
  }, [t, analyzePath]);

  /* Drag & drop папки из файлового менеджера */
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    (async () => {
      unlisten = await getCurrentWebview().onDragDropEvent(async (event) => {
        const type = event.payload.type;
        if (type === "over") return; // событие идёт непрерывно
        if (type === "enter") {
          if (screenRef.current === "dashboard" && !analyzingRef.current) {
            setDragOver(true);
          }
          return;
        }
        if (type === "leave") {
          setDragOver(false);
          return;
        }
        // drop
        setDragOver(false);
        if (screenRef.current !== "dashboard" || analyzingRef.current) return;
        const path = event.payload.paths[0];
        if (!path) return;
        try {
          const isDir = await invoke<boolean>("is_directory", { path });
          if (!isDir) {
            setError(tRef.current("drop.notFolder"));
            return;
          }
          await analyzePathRef.current(path);
        } catch (e) {
          setError(String(e));
        }
      });
    })();
    return () => unlisten?.();
  }, []);

  const startCompression = useCallback(async () => {
    if (!gamePath) return;
    setError(null);
    try {
      await invoke("start_compression", {
        path: gamePath,
        algorithm: effAlgo,
      });
      // Сжатие запущено — статус «устарело» для этой папки больше не актуален
      setStaleness((prev) => {
        const { [gamePath]: _, ...rest } = prev;
        return rest;
      });
      setProgress(null);
      setFileLog([]);
      setScreen("working");
    } catch (e) {
      setError(String(e));
    }
  }, [gamePath, effAlgo]);

  /* Дожать папку из истории: открываем экран анализа (с выбором уровня),
     предвыбрав алгоритм прошлого сжатия */
  const recompress = useCallback(
    async (h: HistoryEntry) => {
      if (h.algorithm && ALGO_IDS.includes(h.algorithm)) {
        changeAlgo(h.algorithm);
      }
      await analyzePath(h.root);
    },
    [analyzePath, changeAlgo],
  );

  const deleteHistoryEntry = useCallback(
    async (root: string) => {
      try {
        await invoke("remove_history_entry", { root });
        void refreshHistory();
      } catch (e) {
        setError(String(e));
      }
    },
    [refreshHistory],
  );

  const openInExplorer = useCallback(async (path: string) => {
    try {
      await invoke("open_in_explorer", { path });
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const startDecompression = useCallback(
    async (path?: string) => {
      const target = path ?? gamePath;
      if (!target) return;
      setError(null);
      setGamePath(target);
      try {
        await invoke<AnalysisSummary>("analyze_folder", { path: target });
        await invoke("start_decompression", { path: target });
        setProgress(null);
        setFileLog([]);
        setScreen("working");
      } catch (e) {
        setError(String(e));
        void refreshHistory();
      }
    },
    [gamePath, refreshHistory],
  );

  const togglePause = useCallback(async () => {
    if (progress?.state === "paused") await invoke("resume_job");
    else await invoke("pause_job");
  }, [progress]);

  const cancelJob = useCallback(async () => {
    await invoke("cancel_job");
  }, []);

  const reset = useCallback(() => {
    setScreen("dashboard");
    setDone(null);
    setProgress(null);
    setFileLog([]);
  }, []);

  const freePct = disk
    ? Math.round((disk.free_bytes / Math.max(disk.total_bytes, 1)) * 100)
    : 0;

  const paused = progress?.state === "paused";
  const cancelling = progress?.state === "cancelling";
  const savedSmooth = useSmoothNumber(
    Math.max(progress?.saved_bytes ?? 0, 0),
    0.2,
  );

  const dfmt = useMemo(
    () =>
      new Intl.DateTimeFormat(numberLocale(lang), {
        dateStyle: "medium",
        timeStyle: "short",
      }),
    [lang],
  );

  /* Прогноз с учётом выбранного уровня сжатия (WOF/Btrfs) */
  const estimatedBytes = analysis
    ? (algoChoices.length > 0
        ? analysis.estimated_bytes_by_algo?.[effAlgo]
        : undefined) ?? analysis.estimated_bytes
    : 0;
  const estimatedSavingsPct = analysis
    ? Math.round(
        Math.max(0, 1 - estimatedBytes / Math.max(analysis.total_bytes, 1)) *
          100,
      )
    : 0;

  /* Папки из истории — для бейджа «сжато» в библиотеке */
  const compressedRoots = useMemo(
    () => new Set(history.map((h) => h.root.toLowerCase())),
    [history],
  );

  /* Сколько записей истории требуют дожатия — точка-индикатор на вкладке */
  const staleCount = useMemo(
    () =>
      Object.values(staleness).filter((s) => s.status === "stale").length,
    [staleness],
  );

  return (
    <div className="app-bg flex h-full flex-col overflow-hidden">
      {/* ---------- Шапка ---------- */}
      <header className="flex items-center justify-between border-b border-[--border] px-7 py-4">
        <div className="flex items-center gap-3.5">
          <LogoMark />
          <div>
            <div className="text-[15px] font-semibold tracking-tight text-[--text-1]">
              Game Compressor
            </div>
            <div className="label mt-0.5">{t("app.subtitle")}</div>
          </div>
        </div>

        <div className="flex items-center gap-2.5">
          {gamePath && (
            <div className="font-mono max-w-[300px] truncate rounded-lg border border-[--border] bg-[--surface] px-3 py-1.5 text-[11px] text-[--text-2]">
              {gamePath}
            </div>
          )}
          <div className="relative">
            <Btn onClick={() => setLangOpen((v) => !v)} className="!px-3">
              <Languages className="h-4 w-4 text-[--text-2]" />
              <span className="text-xs font-semibold uppercase tracking-wider text-[--text-2]">
                {lang}
              </span>
            </Btn>
            <AnimatePresence>
              {langOpen && (
                <motion.div
                  initial={{ opacity: 0, y: -6, scale: 0.97 }}
                  animate={{ opacity: 1, y: 0, scale: 1 }}
                  exit={{ opacity: 0, y: -6, scale: 0.97 }}
                  transition={{ duration: 0.18, ease: EASE }}
                  className="card !absolute right-0 top-11 z-50 w-52 p-1.5"
                >
                  <div className="label px-2.5 pb-1.5 pt-1.5">
                    {t("settings.language")}
                  </div>
                  {LANGS.map((l) => (
                    <button
                      key={l.code}
                      onClick={() => {
                        changeLang(l.code);
                        setLangOpen(false);
                      }}
                      className={`flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[13px] transition-colors ${
                        lang === l.code
                          ? "bg-white/[0.06] text-[--text-1]"
                          : "text-[--text-2] hover:bg-white/[0.04] hover:text-[--text-1]"
                      }`}
                    >
                      <span className="font-mono w-6 text-[10px] font-semibold uppercase text-[--text-3]">
                        {l.code}
                      </span>
                      {l.label}
                      {lang === l.code && (
                        <Check className="ml-auto h-3.5 w-3.5 text-[--accent-a]" />
                      )}
                    </button>
                  ))}
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </div>
      </header>

      {/* ---------- Оверлей drag & drop ---------- */}
      <AnimatePresence>
        {dragOver && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="pointer-events-none fixed inset-0 z-40 grid place-items-center bg-black/40 p-6"
          >
            <div className="card card-accent flex items-center gap-4 border-2 border-dashed border-[--accent-a] px-10 py-8">
              <FolderOpen className="h-6 w-6 text-[--accent-a]" />
              <span className="text-[15px] font-semibold text-[--text-1]">
                {t("drop.hint")}
              </span>
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* ---------- Контент ---------- */}
      {/* scrollbar-gutter: stable — чтобы появление скроллбара не сдвигало
          центрированный контент (прыжок вёрстки при открытии дропдауна) */}
      <main
        ref={mainRef}
        className="flex-1 overflow-y-auto px-7 py-8 [scrollbar-gutter:stable_both-edges]"
        onClick={() => langOpen && setLangOpen(false)}
      >
        <AnimatePresence>
          {error && (
            <motion.div
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              className="mx-auto mb-5 max-w-3xl"
            >
              <div className="card flex items-start gap-3 border-[rgba(242,109,128,0.3)] px-4 py-3 text-[13px] text-[--danger]">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                <span className="select-text">{error}</span>
                <button
                  onClick={() => setError(null)}
                  className="ml-auto text-[--text-3] transition-colors hover:text-[--text-1]"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        <AnimatePresence mode="wait">
          {/* ============ ЭКРАН 1-2: дашборд + анализ ============ */}
          {screen === "dashboard" && (
            <motion.div
              key="dashboard"
              variants={screenVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="mx-auto flex max-w-3xl flex-col gap-4"
            >
              {/* Вкладки: Сжатие / Библиотека / История */}
              <motion.div variants={itemVariants} className="flex gap-1.5">
                {DASH_TABS.map((tab) => (
                  <button
                    key={tab}
                    onClick={() => setDashTab(tab)}
                    className={`relative rounded-xl border px-4 py-2 text-[12.5px] font-semibold transition-colors ${
                      dashTab === tab
                        ? "border-[--accent-a] bg-white/[0.06] text-[--text-1]"
                        : "border-[--border] bg-[--surface] text-[--text-3] hover:text-[--text-1]"
                    }`}
                  >
                    {t(`tabs.${tab}`)}
                    {tab === "library" && library.length > 0 && (
                      <span className="display-num ml-1.5 text-[10px] text-[--text-3]">
                        {library.length}
                      </span>
                    )}
                    {tab === "history" && history.length > 0 && (
                      <span className="display-num ml-1.5 text-[10px] text-[--text-3]">
                        {history.length}
                      </span>
                    )}
                    {tab === "history" && staleCount > 0 && (
                      <span className="absolute -right-1 -top-1 h-2.5 w-2.5 rounded-full bg-[--warning]" />
                    )}
                  </button>
                ))}
              </motion.div>

              {dashTab === "compress" && (
                <>
              {/* Hero: выбор папки */}
              <motion.div
                variants={itemVariants}
                className="card card-accent p-7"
              >
                <div className="flex flex-wrap items-center justify-between gap-5">
                  <div>
                    <h2 className="text-[17px] font-semibold tracking-tight">
                      {t("dash.folderTitle")}
                    </h2>
                    <p className="mt-1 max-w-sm text-[13px] leading-relaxed text-[--text-2]">
                      {t("dash.folderDesc")}
                    </p>
                  </div>
                  <Btn onClick={pickFolder} disabled={analyzing} kind="primary">
                    <FolderOpen className="h-4 w-4" />
                    {t("dash.pick")}
                  </Btn>
                </div>
              </motion.div>

              {/* Карточка диска */}
              {disk && (
                <motion.div
                  variants={itemVariants}
                  initial="initial"
                  animate="animate"
                  className="card p-6"
                >
                  <div className="flex items-center gap-3">
                    <div className="grid h-9 w-9 place-items-center rounded-xl border border-[--border] bg-[--surface]">
                      <HardDrive className="h-4 w-4 text-[--text-2]" />
                    </div>
                    <div>
                      <div className="text-[14px] font-semibold">
                        {t("disk.title", { mp: disk.mount_point })}
                      </div>
                      <div className="text-[11.5px] text-[--text-3]">
                        {t("disk.free", {
                          p: freePct,
                          free: fmtBytes(disk.free_bytes),
                          total: fmtBytes(disk.total_bytes),
                        })}
                      </div>
                    </div>
                    <span className="font-mono ml-auto rounded-md border border-[--border] bg-[--surface] px-2.5 py-1 text-[10.5px] font-semibold tracking-wider text-[--text-2]">
                      {disk.filesystem}
                    </span>
                  </div>

                  <div className="meter mt-5">
                    <motion.div
                      className="meter-fill"
                      initial={{ width: 0 }}
                      animate={{ width: `${100 - freePct}%` }}
                      transition={{ duration: 0.8, ease: EASE }}
                    />
                  </div>

                  <div
                    className={`mt-5 flex items-start gap-3 rounded-xl border px-4 py-3.5 text-[13px] leading-relaxed ${
                      disk.supported
                        ? "border-[rgba(63,211,148,0.25)] bg-[rgba(63,211,148,0.05)] text-[#a9edcf]"
                        : "border-[rgba(242,109,128,0.3)] bg-[rgba(242,109,128,0.05)] text-[#ffc9d1]"
                    }`}
                  >
                    {disk.supported ? (
                      <ShieldCheck className="mt-0.5 h-4.5 w-4.5 shrink-0 text-[--success]" />
                    ) : (
                      <AlertTriangle className="mt-0.5 h-4.5 w-4.5 shrink-0 text-[--danger]" />
                    )}
                    <div>
                      {!disk.supported && (
                        <div className="mb-1 text-[11px] font-bold uppercase tracking-widest">
                          {t("disk.blocked")}
                        </div>
                      )}
                      {t(`fs.${disk.reason_code}`, { fs: disk.filesystem })}
                    </div>
                  </div>
                </motion.div>
              )}

              {/* Сканирование */}
              <AnimatePresence>
                {analyzing && (
                  <motion.div
                    initial={{ opacity: 0, y: 14 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -8 }}
                    className="card flex items-center gap-4 p-6"
                  >
                    <Loader2 className="h-5 w-5 animate-spin text-[--accent-a]" />
                    <div>
                      <div className="text-[14px] font-semibold">
                        {t("scan.title")}
                      </div>
                      <div className="display-num text-[12px] text-[--text-3]">
                        {t("scan.found", { n: fmtInt(scannedFiles) })}
                      </div>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              {/* Анализ и прогноз */}
              {analysis && disk?.supported && (
                <motion.div
                  variants={itemVariants}
                  initial="initial"
                  animate="animate"
                  className="flex flex-col gap-4"
                >
                  <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
                    <Stat
                      label={t("analysis.files")}
                      value={fmtInt(analysis.total_files)}
                    />
                    <Stat
                      label={t("analysis.totalSize")}
                      value={fmtBytes(analysis.total_bytes)}
                    />
                    <Stat
                      label={t("analysis.after")}
                      value={`~${fmtBytes(estimatedBytes)}`}
                      tone="accent"
                    />
                    <Stat
                      label={t("analysis.savings")}
                      value={`~${estimatedSavingsPct}%`}
                      tone="success"
                    />
                  </div>

                  {analysis.proton_hint && (
                    <div className="card flex items-start gap-3 px-4 py-3.5 text-[12.5px] leading-relaxed text-[--text-2]">
                      <Gamepad2 className="mt-0.5 h-4 w-4 shrink-0 text-[--accent-a]" />
                      {t("analysis.protonNote")}
                    </div>
                  )}

                  {analysis.already_compressed_files > 0 &&
                    analysis.compressible_files > 0 && (
                      <div className="card flex items-start gap-3 px-4 py-3.5 text-[12.5px] leading-relaxed text-[--text-2]">
                        <ArchiveRestore className="mt-0.5 h-4 w-4 shrink-0 text-[--success]" />
                        {t("analysis.already", {
                          n: fmtInt(analysis.already_compressed_files),
                          v: fmtBytes(analysis.already_saved_bytes),
                        })}
                      </div>
                    )}

                  {analysis.compressible_files === 0 &&
                  analysis.already_compressed_files > 0 ? (
                    /* Папка уже полностью сжата — предлагаем только откат */
                    <div className="card flex flex-wrap items-center justify-between gap-4 border-[rgba(63,211,148,0.25)] p-6">
                      <div className="flex items-start gap-3">
                        <ShieldCheck className="mt-0.5 h-4.5 w-4.5 shrink-0 text-[--success]" />
                        <p className="max-w-md text-[13px] leading-relaxed text-[#a9edcf]">
                          {t("analysis.allCompressed")}
                        </p>
                      </div>
                      <Btn onClick={() => startDecompression()} kind="danger">
                        <ArchiveRestore className="h-4 w-4" />
                        {t("done.restore")}
                      </Btn>
                    </div>
                  ) : (
                    <div className="card flex flex-col gap-5 p-6">
                      {/* Уровень сжатия: WOF на NTFS, zstd/zlib/lzo на Btrfs */}
                      {algoChoices.length > 0 && (
                        <div>
                          <div className="label">{t("level.title")}</div>
                          <div className="mt-2 flex flex-wrap items-center gap-1.5">
                            {algoChoices.map((a) => (
                              <button
                                key={a}
                                onClick={() => changeAlgo(a)}
                                className={`font-mono rounded-lg border px-3 py-1.5 text-[11px] font-semibold tracking-wider transition-colors ${
                                  effAlgo === a
                                    ? "border-[--accent-a] bg-white/[0.06] text-[--text-1]"
                                    : "border-[--border] bg-[--surface] text-[--text-3] hover:text-[--text-1]"
                                }`}
                              >
                                {ALGO_LABELS[a]}
                              </button>
                            ))}
                          </div>
                          <div className="mt-2 text-[11.5px] text-[--text-3]">
                            {t(`level.desc.${effAlgo}`)}
                          </div>
                        </div>
                      )}
                      <div className="flex flex-wrap items-center justify-between gap-4">
                        <p className="max-w-md text-[12px] leading-relaxed text-[--text-3]">
                          {t("analysis.skippedNote", {
                            n: fmtInt(analysis.skipped_files),
                          })}
                        </p>
                        <Btn
                          onClick={startCompression}
                          disabled={analysis.compressible_files === 0}
                          kind="primary"
                        >
                          <ChevronsDownUp className="h-4 w-4" />
                          {t("analysis.start")}
                        </Btn>
                      </div>
                    </div>
                  )}
                </motion.div>
              )}
                </>
              )}

              {/* История сжатий */}
              {dashTab === "history" && (
                <motion.div
                  variants={itemVariants}
                  initial="initial"
                  animate="animate"
                  className="card p-6"
                >
                  <div className="flex items-center gap-3">
                    <div className="grid h-9 w-9 place-items-center rounded-xl border border-[--border] bg-[--surface]">
                      <History className="h-4 w-4 text-[--text-2]" />
                    </div>
                    <div className="text-[14px] font-semibold">
                      {t("history.title")}
                    </div>
                    {checkingStale && (
                      <span className="ml-auto flex items-center gap-1.5 text-[11px] text-[--text-3]">
                        <Loader2 className="h-3 w-3 animate-spin" />
                        {t("history.checking")}
                      </span>
                    )}
                  </div>
                  {history.length === 0 && (
                    <p className="mt-4 text-[12.5px] text-[--text-3]">
                      {t("history.empty")}
                    </p>
                  )}
                  <div className="mt-4 flex flex-col gap-2">
                    {history.map((h) => {
                      const st = staleness[h.root];
                      const isStale = st?.status === "stale";
                      const isMissing = st?.status === "missing";
                      return (
                        <div
                          key={h.root}
                          className="flex items-center gap-4 rounded-xl border border-[--border] bg-[--surface] px-4 py-3 transition-colors hover:bg-white/[0.04]"
                        >
                          <div className="min-w-0 flex-1">
                            <div className="font-mono truncate text-[12px] text-[--text-1]">
                              {h.root}
                            </div>
                            <div className="mt-0.5 text-[11px] text-[--text-3]">
                              {dfmt.format(h.date * 1000)} ·{" "}
                              {t("history.meta", {
                                n: fmtInt(h.files),
                                v: fmtBytes(Math.max(h.saved_bytes, 0)),
                              })}
                              {h.algorithm && (
                                <span className="font-mono ml-1.5 rounded border border-[--border] px-1.5 py-0.5 text-[9.5px] uppercase tracking-wider text-[--text-3]">
                                  {ALGO_LABELS[h.algorithm]}
                                </span>
                              )}
                              {h.partial && (
                                <span className="ml-1.5 rounded bg-[rgba(232,180,90,0.12)] px-1.5 py-0.5 text-[10px] text-[--warning]">
                                  {t("history.partial")}
                                </span>
                              )}
                              {isStale && (
                                <span className="ml-1.5 rounded bg-[rgba(232,180,90,0.12)] px-1.5 py-0.5 text-[10px] text-[--warning]">
                                  {t("history.stale")}
                                </span>
                              )}
                              {isMissing && (
                                <span className="ml-1.5 rounded bg-[rgba(242,109,128,0.12)] px-1.5 py-0.5 text-[10px] text-[--danger]">
                                  {t("history.missing")}
                                </span>
                              )}
                            </div>
                            {isStale && (
                              <div className="mt-0.5 text-[11px] text-[--warning]">
                                {t("history.staleHint", {
                                  v: fmtBytes(st.potential_saved_bytes),
                                })}
                              </div>
                            )}
                          </div>
                          {isStale && (
                            <Btn
                              onClick={() => recompress(h)}
                              kind="primary"
                              className="shrink-0 !px-3.5 !py-2 !text-[12px]"
                            >
                              <RefreshCw className="h-3.5 w-3.5" />
                              {t("history.recompress")}
                            </Btn>
                          )}
                          <Btn
                            onClick={() => startDecompression(h.root)}
                            disabled={isMissing}
                            className="shrink-0 !px-3.5 !py-2 !text-[12px]"
                          >
                            <ArchiveRestore className="h-3.5 w-3.5" />
                            {t("history.restore")}
                          </Btn>
                          <button
                            onClick={() => openInExplorer(h.root)}
                            disabled={isMissing}
                            title={t("history.open")}
                            className="shrink-0 rounded-lg p-2 text-[--text-3] transition-colors hover:bg-white/[0.06] hover:text-[--text-1] disabled:opacity-30"
                          >
                            <FolderSearch className="h-4 w-4" />
                          </button>
                          <button
                            onClick={() => deleteHistoryEntry(h.root)}
                            title={t("history.delete")}
                            className="shrink-0 rounded-lg p-2 text-[--text-3] transition-colors hover:bg-white/[0.06] hover:text-[--danger]"
                          >
                            <Trash2 className="h-4 w-4" />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                </motion.div>
              )}

              {/* Библиотека игр из лаунчеров */}
              {dashTab === "library" && (
                <motion.div
                  variants={itemVariants}
                  initial="initial"
                  animate="animate"
                  className="card p-6"
                >
                  <div className="flex items-center gap-3">
                    <div className="grid h-9 w-9 place-items-center rounded-xl border border-[--border] bg-[--surface]">
                      <Gamepad2 className="h-4 w-4 text-[--text-2]" />
                    </div>
                    <div className="text-[14px] font-semibold">
                      {t("library.title")}
                    </div>
                    <span className="ml-auto text-[11px] text-[--text-3]">
                      {t("library.count", { n: fmtInt(library.length) })}
                    </span>
                  </div>
                  {library.length === 0 && (
                    <p className="mt-4 text-[12.5px] text-[--text-3]">
                      {t("library.empty")}
                    </p>
                  )}
                  <div className="mt-4 grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-5">
                    {library.map((g) => {
                      const done = compressedRoots.has(g.path.toLowerCase());
                      const selected = gamePath === g.path;
                      return (
                        <button
                          key={g.path}
                          onClick={() => analyzePath(g.path)}
                          disabled={analyzing}
                          title={g.path}
                          className="group flex flex-col gap-1.5 text-left disabled:opacity-50"
                        >
                          <div
                            className={`relative aspect-[2/3] w-full overflow-hidden rounded-xl border bg-[--surface] transition-all ${
                              selected
                                ? "border-[--accent-a] shadow-[0_0_0_2px_rgba(124,127,242,0.35)]"
                                : "border-[--border] group-hover:border-[--border-strong]"
                            }`}
                          >
                            {g.cover ? (
                              <img
                                src={g.cover}
                                alt={g.name}
                                loading="lazy"
                                className="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.04]"
                              />
                            ) : (
                              <div className="grid h-full w-full place-items-center p-2">
                                <Gamepad2 className="h-6 w-6 text-[--text-3]" />
                                <span className="line-clamp-3 text-center text-[10.5px] leading-snug text-[--text-2]">
                                  {g.name}
                                </span>
                              </div>
                            )}
                            <span className="font-mono absolute left-1.5 top-1.5 rounded bg-black/60 px-1.5 py-0.5 text-[8.5px] font-semibold uppercase tracking-wider text-[--text-2] backdrop-blur">
                              {g.launcher}
                            </span>
                            {done && (
                              <span className="absolute bottom-1.5 right-1.5 rounded bg-[rgba(63,211,148,0.85)] px-1.5 py-0.5 text-[8.5px] font-bold uppercase tracking-wider text-black">
                                {t("library.compressed")}
                              </span>
                            )}
                          </div>
                          <span className="truncate text-[11px] text-[--text-2] group-hover:text-[--text-1]">
                            {g.name}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </motion.div>
              )}
            </motion.div>
          )}

          {/* ============ ЭКРАН 3: процесс ============ */}
          {screen === "working" && (
            <motion.div
              key="working"
              variants={screenVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="mx-auto flex max-w-3xl flex-col gap-4"
            >
              <motion.div variants={itemVariants} className="card p-8">
                <div className="flex items-center justify-between">
                  <h2 className="text-[16px] font-semibold tracking-tight">
                    {progress?.mode === "decompress"
                      ? t("work.decompress")
                      : t("work.compress")}
                  </h2>
                  <div
                    className={`flex items-center gap-2.5 rounded-full border border-[--border] bg-[--surface] px-3.5 py-1.5 text-[11px] font-semibold uppercase tracking-widest ${
                      cancelling
                        ? "text-[--danger]"
                        : paused
                          ? "text-[--warning]"
                          : "text-[--accent-b]"
                    }`}
                  >
                    <span
                      className="status-dot"
                      style={{ background: "currentColor" }}
                    />
                    {cancelling
                      ? t("work.cancelling")
                      : paused
                        ? t("work.paused")
                        : t("work.running")}
                  </div>
                </div>

                <div className="mt-6 flex flex-col items-center gap-8 md:flex-row md:gap-10">
                  <Ring percent={progress?.percent ?? 0} paused={paused} />

                  <div className="flex flex-1 flex-col gap-3">
                    <div className="grid grid-cols-2 gap-3">
                      <Stat
                        label={t("work.filesDone")}
                        value={`${fmtInt(progress?.processed_files ?? 0)} / ${fmtInt(progress?.total_files ?? 0)}`}
                      />
                      <Stat
                        label={t("work.speed")}
                        value={`${fmtBytes(paused ? 0 : (progress?.speed_bps ?? 0))}/s`}
                      />
                      <Stat
                        label={t("work.freed")}
                        value={fmtBytes(savedSmooth)}
                        tone="success"
                      />
                      <Stat
                        label={t("work.time")}
                        value={`${Math.floor((progress?.elapsed_secs ?? 0) / 60)}:${String(
                          Math.floor((progress?.elapsed_secs ?? 0) % 60),
                        ).padStart(2, "0")}`}
                      />
                    </div>
                    <div className={`meter ${paused ? "" : "meter-live"}`}>
                      <div
                        className="meter-fill"
                        style={{
                          width: `${Math.min(progress?.percent ?? 0, 100)}%`,
                        }}
                      />
                    </div>
                    <div className="display-num flex justify-between text-[11.5px] text-[--text-3]">
                      <span>{fmtBytes(progress?.bytes_processed ?? 0)}</span>
                      <span>{fmtBytes(progress?.total_bytes ?? 0)}</span>
                    </div>
                  </div>
                </div>

                {/* Лог файлов */}
                <div className="font-mono mt-7 h-[124px] overflow-hidden rounded-xl border border-[--border] bg-black/30 p-3 text-[11px] leading-[1.55]">
                  {fileLog.length === 0 && (
                    <span className="text-[--text-3]">{t("work.waiting")}</span>
                  )}
                  <AnimatePresence initial={false}>
                    {fileLog.map((f, i) => (
                      <motion.div
                        key={f.id}
                        initial={{ opacity: 0, x: -8 }}
                        animate={{ opacity: i === 0 ? 1 : 0.35, x: 0 }}
                        exit={{ opacity: 0 }}
                        transition={{ duration: 0.25 }}
                        className={`truncate ${i === 0 ? "text-[--accent-b]" : "text-[--text-3]"}`}
                      >
                        {f.path}
                      </motion.div>
                    ))}
                  </AnimatePresence>
                </div>

                <div className="mt-7 flex justify-center gap-3">
                  <Btn onClick={togglePause} disabled={cancelling}>
                    {paused ? (
                      <>
                        <Play className="h-4 w-4" /> {t("work.resume")}
                      </>
                    ) : (
                      <>
                        <Pause className="h-4 w-4" /> {t("work.pause")}
                      </>
                    )}
                  </Btn>
                  <Btn onClick={cancelJob} kind="danger" disabled={cancelling}>
                    <Square className="h-3.5 w-3.5" />{" "}
                    {cancelling ? t("work.cancelling") : t("work.cancel")}
                  </Btn>
                </div>
              </motion.div>
            </motion.div>
          )}

          {/* ============ ЭКРАН 4: завершение ============ */}
          {screen === "done" && done && (
            <motion.div
              key="done"
              variants={screenVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              className="mx-auto flex max-w-3xl flex-col gap-4"
            >
              <motion.div
                variants={itemVariants}
                className="card card-accent p-10 text-center"
              >
                <motion.div
                  initial={{ scale: 0.4, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  transition={{
                    type: "spring",
                    stiffness: 260,
                    damping: 18,
                    delay: 0.15,
                  }}
                  className={`mx-auto grid h-16 w-16 place-items-center rounded-2xl border ${
                    done.cancelled
                      ? "border-[rgba(232,180,90,0.35)] bg-[rgba(232,180,90,0.08)]"
                      : "border-[rgba(63,211,148,0.35)] bg-[rgba(63,211,148,0.08)]"
                  }`}
                >
                  {done.cancelled ? (
                    <AlertTriangle className="h-7 w-7 text-[--warning]" />
                  ) : (
                    <Check className="h-7 w-7 text-[--success]" />
                  )}
                </motion.div>

                <h2 className="mt-5 text-[22px] font-bold tracking-tight">
                  {done.cancelled
                    ? t("done.cancelled")
                    : done.mode === "decompress"
                      ? t("done.decompressOk")
                      : t("done.compressOk")}
                </h2>
                <p className="mt-1.5 text-[13px] text-[--text-2]">
                  {t("done.summary", {
                    p: fmtInt(done.processed_files),
                    t: fmtInt(done.total_files),
                    s: Math.round(done.elapsed_secs),
                  })}{" "}
                  {done.failed_files > 0 &&
                    t("done.errors", { n: fmtInt(done.failed_files) })}
                </p>

                {done.mode === "compress" && (
                  <div className="mx-auto mt-7 grid max-w-xl grid-cols-1 gap-3 md:grid-cols-3">
                    <Stat
                      label={t("done.original")}
                      value={fmtBytes(done.original_bytes)}
                    />
                    <Stat
                      label={t("done.freed")}
                      value={fmtBytes(Math.max(done.saved_bytes, 0))}
                      tone="success"
                    />
                    <Stat
                      label={t("done.final")}
                      value={fmtBytes(done.final_physical_bytes)}
                      tone="accent"
                    />
                  </div>
                )}

                {done.errors.length > 0 && (
                  <details className="mx-auto mt-5 max-w-xl text-left">
                    <summary className="cursor-pointer text-[12px] text-[--danger]">
                      {t("done.showErrors", { n: done.errors.length })}
                    </summary>
                    <div className="font-mono mt-2 max-h-32 select-text overflow-y-auto rounded-lg border border-[--border] bg-black/30 p-3 text-[10.5px] leading-relaxed text-[--danger]">
                      {done.errors.map((e, i) => (
                        <div key={i}>{e}</div>
                      ))}
                    </div>
                  </details>
                )}

                <div className="mt-9 flex flex-wrap justify-center gap-3">
                  <Btn onClick={reset}>
                    <ArrowLeft className="h-4 w-4" /> {t("done.home")}
                  </Btn>
                  {done.mode === "compress" && !done.cancelled && (
                    <Btn onClick={() => startDecompression()} kind="danger">
                      <ArchiveRestore className="h-4 w-4" /> {t("done.restore")}
                    </Btn>
                  )}
                </div>
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </main>
    </div>
  );
}
