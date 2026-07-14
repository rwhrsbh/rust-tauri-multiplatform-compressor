/* Интернационализация: словари, выбор языка, подстановка параметров. */

export type Lang = "ru" | "en" | "uk" | "de";

export const LANGS: { code: Lang; label: string }[] = [
  { code: "ru", label: "Русский" },
  { code: "en", label: "English" },
  { code: "uk", label: "Українська" },
  { code: "de", label: "Deutsch" },
];

type Dict = Record<string, string>;

const ru: Dict = {
  "app.subtitle": "Прозрачное сжатие · NTFS / APFS / Btrfs",
  "dash.folderTitle": "Папка с игрой",
  "dash.folderDesc":
    "Выберите папку — мы проверим диск и оценим потенциал сжатия.",
  "dash.pick": "Выбрать папку с игрой",
  "dialog.pickTitle": "Выберите папку с игрой",
  "disk.title": "Диск {mp}",
  "disk.used": "Занято {v}",
  "disk.free": "{p}% свободно · {free} из {total}",
  "disk.blocked": "Внимание! Дальнейшие действия заблокированы",
  "fs.ok.ntfs":
    "Файловая система подходит. Прозрачное сжатие поддерживается (NTFS: WOF/LZX с откатом на LZNT1).",
  "fs.ok.apfs":
    "Файловая система подходит. Прозрачное сжатие поддерживается (APFS/HFS+: decmpfs).",
  "fs.ok.btrfs":
    "Файловая система подходит. Прозрачное сжатие поддерживается (Btrfs: zstd). Работает и для Proton/Wine-игр.",
  "fs.bad.fat":
    "Файловая система {fs} не поддерживает сжатие. Перенесите игру на диск с NTFS (Windows), APFS (macOS) или Btrfs (Linux).",
  "fs.bad.generic":
    "Выбранная папка находится на диске с файловой системой {fs}. Данная ФС не поддерживает сжатие на лету. Перенесите игру на диск с NTFS (Windows), APFS (macOS) или Btrfs (Linux).",
  "scan.title": "Сканирование файлов…",
  "scan.found": "Найдено файлов: {n}",
  "analysis.title": "Анализ и прогноз сжатия",
  "analysis.files": "Файлов",
  "analysis.totalSize": "Общий размер",
  "analysis.after": "После сжатия",
  "analysis.savings": "Экономия",
  "analysis.skippedNote":
    "Пропущено {n} файлов (исполняемые .exe/.dll/.dylib/.so и мелкие файлы) — для совместимости с античитами и лаунчерами.",
  "analysis.protonNote":
    "Обнаружена Windows-игра (Proton/Wine). Сжатие безопасно: файлы лежат в файловой системе хоста, а исполняемые файлы и библиотеки не затрагиваются.",
  "analysis.start": "Начать сжатие",
  "work.compress": "Сжатие в процессе",
  "work.decompress": "Восстановление файлов",
  "work.running": "Работает",
  "work.paused": "Пауза",
  "work.filesDone": "Сжато файлов",
  "work.speed": "Скорость",
  "work.freed": "Освобождено",
  "work.time": "Время",
  "work.waiting": "ожидание данных…",
  "work.pause": "Пауза",
  "work.resume": "Продолжить",
  "work.cancel": "Отмена",
  "work.cancelling": "Отмена…",
  "done.cancelled": "Операция отменена",
  "done.compressOk": "Успешно сжато!",
  "done.decompressOk": "Файлы восстановлены!",
  "done.summary": "Обработано {p} из {t} файлов за {s} с",
  "done.errors": "· ошибок: {n}",
  "done.original": "Исходный размер",
  "done.freed": "Освобождено",
  "done.final": "Итог на диске",
  "done.showErrors": "Показать ошибки ({n})",
  "done.home": "На главную",
  "done.restore": "Вернуть в исходное состояние",
  "settings.title": "Настройки",
  "settings.language": "Язык интерфейса",
  "history.title": "История сжатий",
  "history.restore": "Восстановить",
  "history.meta": "{n} файлов · освобождено {v}",
  "history.partial": "частично",
  "analysis.already":
    "{n} файлов уже сжаты ранее (сэкономлено {v}) — они будут пропущены.",
  "analysis.allCompressed":
    "Эта папка уже полностью сжата. Повторное сжатие не требуется — можно вернуть её в исходное состояние.",
};

const en: Dict = {
  "app.subtitle": "Transparent compression · NTFS / APFS / Btrfs",
  "dash.folderTitle": "Game folder",
  "dash.folderDesc":
    "Pick a folder — we'll check the drive and estimate compression potential.",
  "dash.pick": "Select game folder",
  "dialog.pickTitle": "Select the game folder",
  "disk.title": "Drive {mp}",
  "disk.used": "Used {v}",
  "disk.free": "{p}% free · {free} of {total}",
  "disk.blocked": "Warning! Further actions are blocked",
  "fs.ok.ntfs":
    "Filesystem is suitable. Transparent compression is supported (NTFS: WOF/LZX with LZNT1 fallback).",
  "fs.ok.apfs":
    "Filesystem is suitable. Transparent compression is supported (APFS/HFS+: decmpfs).",
  "fs.ok.btrfs":
    "Filesystem is suitable. Transparent compression is supported (Btrfs: zstd). Works for Proton/Wine games too.",
  "fs.bad.fat":
    "The {fs} filesystem does not support compression. Move the game to a drive with NTFS (Windows), APFS (macOS) or Btrfs (Linux).",
  "fs.bad.generic":
    "The selected folder is on a drive with the {fs} filesystem. It does not support on-the-fly compression. Move the game to a drive with NTFS (Windows), APFS (macOS) or Btrfs (Linux).",
  "scan.title": "Scanning files…",
  "scan.found": "Files found: {n}",
  "analysis.title": "Analysis & compression estimate",
  "analysis.files": "Files",
  "analysis.totalSize": "Total size",
  "analysis.after": "After compression",
  "analysis.savings": "Savings",
  "analysis.skippedNote":
    "{n} files skipped (executables .exe/.dll/.dylib/.so and tiny files) — for anti-cheat and launcher compatibility.",
  "analysis.protonNote":
    "Windows game detected (Proton/Wine). Compression is safe: files live on the host filesystem and executables/libraries are left untouched.",
  "analysis.start": "Start compression",
  "work.compress": "Compression in progress",
  "work.decompress": "Restoring files",
  "work.running": "Running",
  "work.paused": "Paused",
  "work.filesDone": "Files compressed",
  "work.speed": "Speed",
  "work.freed": "Space freed",
  "work.time": "Time",
  "work.waiting": "waiting for data…",
  "work.pause": "Pause",
  "work.resume": "Resume",
  "work.cancel": "Cancel",
  "work.cancelling": "Cancelling…",
  "done.cancelled": "Operation cancelled",
  "done.compressOk": "Compressed successfully!",
  "done.decompressOk": "Files restored!",
  "done.summary": "Processed {p} of {t} files in {s} s",
  "done.errors": "· errors: {n}",
  "done.original": "Original size",
  "done.freed": "Space freed",
  "done.final": "Final on-disk size",
  "done.showErrors": "Show errors ({n})",
  "done.home": "Back to start",
  "done.restore": "Restore original state",
  "settings.title": "Settings",
  "settings.language": "Interface language",
  "history.title": "Compression history",
  "history.restore": "Restore",
  "history.meta": "{n} files · {v} freed",
  "history.partial": "partial",
  "analysis.already":
    "{n} files were already compressed earlier ({v} saved) — they will be skipped.",
  "analysis.allCompressed":
    "This folder is already fully compressed. No re-compression needed — you can restore it to its original state.",
};

const uk: Dict = {
  "app.subtitle": "Прозоре стиснення · NTFS / APFS / Btrfs",
  "dash.folderTitle": "Тека з грою",
  "dash.folderDesc":
    "Оберіть теку — ми перевіримо диск і оцінимо потенціал стиснення.",
  "dash.pick": "Обрати теку з грою",
  "dialog.pickTitle": "Оберіть теку з грою",
  "disk.title": "Диск {mp}",
  "disk.used": "Зайнято {v}",
  "disk.free": "{p}% вільно · {free} із {total}",
  "disk.blocked": "Увага! Подальші дії заблоковано",
  "fs.ok.ntfs":
    "Файлова система підходить. Прозоре стиснення підтримується (NTFS: WOF/LZX з відкатом на LZNT1).",
  "fs.ok.apfs":
    "Файлова система підходить. Прозоре стиснення підтримується (APFS/HFS+: decmpfs).",
  "fs.ok.btrfs":
    "Файлова система підходить. Прозоре стиснення підтримується (Btrfs: zstd). Працює і для ігор через Proton/Wine.",
  "fs.bad.fat":
    "Файлова система {fs} не підтримує стиснення. Перенесіть гру на диск із NTFS (Windows), APFS (macOS) або Btrfs (Linux).",
  "fs.bad.generic":
    "Обрана тека розташована на диску з файловою системою {fs}. Ця ФС не підтримує стиснення на льоту. Перенесіть гру на диск із NTFS (Windows), APFS (macOS) або Btrfs (Linux).",
  "scan.title": "Сканування файлів…",
  "scan.found": "Знайдено файлів: {n}",
  "analysis.title": "Аналіз і прогноз стиснення",
  "analysis.files": "Файлів",
  "analysis.totalSize": "Загальний розмір",
  "analysis.after": "Після стиснення",
  "analysis.savings": "Економія",
  "analysis.skippedNote":
    "Пропущено {n} файлів (виконувані .exe/.dll/.dylib/.so та дрібні файли) — для сумісності з античитами й лаунчерами.",
  "analysis.protonNote":
    "Виявлено Windows-гру (Proton/Wine). Стиснення безпечне: файли лежать у файловій системі хоста, а виконувані файли та бібліотеки не змінюються.",
  "analysis.start": "Почати стиснення",
  "work.compress": "Стиснення триває",
  "work.decompress": "Відновлення файлів",
  "work.running": "Працює",
  "work.paused": "Пауза",
  "work.filesDone": "Стиснено файлів",
  "work.speed": "Швидкість",
  "work.freed": "Звільнено",
  "work.time": "Час",
  "work.waiting": "очікування даних…",
  "work.pause": "Пауза",
  "work.resume": "Продовжити",
  "work.cancel": "Скасувати",
  "work.cancelling": "Скасування…",
  "done.cancelled": "Операцію скасовано",
  "done.compressOk": "Успішно стиснено!",
  "done.decompressOk": "Файли відновлено!",
  "done.summary": "Оброблено {p} із {t} файлів за {s} с",
  "done.errors": "· помилок: {n}",
  "done.original": "Початковий розмір",
  "done.freed": "Звільнено",
  "done.final": "Підсумок на диску",
  "done.showErrors": "Показати помилки ({n})",
  "done.home": "На головну",
  "done.restore": "Повернути початковий стан",
  "settings.title": "Налаштування",
  "settings.language": "Мова інтерфейсу",
  "history.title": "Історія стиснень",
  "history.restore": "Відновити",
  "history.meta": "{n} файлів · звільнено {v}",
  "history.partial": "частково",
  "analysis.already":
    "{n} файлів вже стиснено раніше (заощаджено {v}) — їх буде пропущено.",
  "analysis.allCompressed":
    "Ця тека вже повністю стиснена. Повторне стиснення не потрібне — можна повернути її до початкового стану.",
};

const de: Dict = {
  "app.subtitle": "Transparente Kompression · NTFS / APFS / Btrfs",
  "dash.folderTitle": "Spielordner",
  "dash.folderDesc":
    "Ordner auswählen — wir prüfen das Laufwerk und schätzen das Kompressionspotenzial.",
  "dash.pick": "Spielordner auswählen",
  "dialog.pickTitle": "Spielordner auswählen",
  "disk.title": "Laufwerk {mp}",
  "disk.used": "Belegt {v}",
  "disk.free": "{p}% frei · {free} von {total}",
  "disk.blocked": "Achtung! Weitere Aktionen sind blockiert",
  "fs.ok.ntfs":
    "Dateisystem ist geeignet. Transparente Kompression wird unterstützt (NTFS: WOF/LZX mit LZNT1-Fallback).",
  "fs.ok.apfs":
    "Dateisystem ist geeignet. Transparente Kompression wird unterstützt (APFS/HFS+: decmpfs).",
  "fs.ok.btrfs":
    "Dateisystem ist geeignet. Transparente Kompression wird unterstützt (Btrfs: zstd). Funktioniert auch für Proton/Wine-Spiele.",
  "fs.bad.fat":
    "Das Dateisystem {fs} unterstützt keine Kompression. Verschieben Sie das Spiel auf ein Laufwerk mit NTFS (Windows), APFS (macOS) oder Btrfs (Linux).",
  "fs.bad.generic":
    "Der gewählte Ordner liegt auf einem Laufwerk mit dem Dateisystem {fs}. Es unterstützt keine On-the-fly-Kompression. Verschieben Sie das Spiel auf ein Laufwerk mit NTFS (Windows), APFS (macOS) oder Btrfs (Linux).",
  "scan.title": "Dateien werden gescannt…",
  "scan.found": "Gefundene Dateien: {n}",
  "analysis.title": "Analyse & Kompressionsprognose",
  "analysis.files": "Dateien",
  "analysis.totalSize": "Gesamtgröße",
  "analysis.after": "Nach Kompression",
  "analysis.savings": "Ersparnis",
  "analysis.skippedNote":
    "{n} Dateien übersprungen (ausführbare .exe/.dll/.dylib/.so und kleine Dateien) — für Anti-Cheat- und Launcher-Kompatibilität.",
  "analysis.protonNote":
    "Windows-Spiel erkannt (Proton/Wine). Die Kompression ist sicher: Dateien liegen im Host-Dateisystem, ausführbare Dateien und Bibliotheken bleiben unberührt.",
  "analysis.start": "Kompression starten",
  "work.compress": "Kompression läuft",
  "work.decompress": "Dateien werden wiederhergestellt",
  "work.running": "Läuft",
  "work.paused": "Pausiert",
  "work.filesDone": "Komprimierte Dateien",
  "work.speed": "Geschwindigkeit",
  "work.freed": "Freigegeben",
  "work.time": "Zeit",
  "work.waiting": "warte auf Daten…",
  "work.pause": "Pause",
  "work.resume": "Fortsetzen",
  "work.cancel": "Abbrechen",
  "work.cancelling": "Wird abgebrochen…",
  "done.cancelled": "Vorgang abgebrochen",
  "done.compressOk": "Erfolgreich komprimiert!",
  "done.decompressOk": "Dateien wiederhergestellt!",
  "done.summary": "{p} von {t} Dateien in {s} s verarbeitet",
  "done.errors": "· Fehler: {n}",
  "done.original": "Ursprüngliche Größe",
  "done.freed": "Freigegeben",
  "done.final": "Endgröße auf der Festplatte",
  "done.showErrors": "Fehler anzeigen ({n})",
  "done.home": "Zur Startseite",
  "done.restore": "Ursprungszustand wiederherstellen",
  "settings.title": "Einstellungen",
  "settings.language": "Sprache der Oberfläche",
  "history.title": "Komprimierungsverlauf",
  "history.restore": "Wiederherstellen",
  "history.meta": "{n} Dateien · {v} freigegeben",
  "history.partial": "teilweise",
  "analysis.already":
    "{n} Dateien wurden bereits komprimiert ({v} gespart) — sie werden übersprungen.",
  "analysis.allCompressed":
    "Dieser Ordner ist bereits vollständig komprimiert. Keine erneute Komprimierung nötig — Sie können den Ursprungszustand wiederherstellen.",
};

const dicts: Record<Lang, Dict> = { ru, en, uk, de };

const STORAGE_KEY = "gc-lang";

export function detectLang(): Lang {
  const saved = localStorage.getItem(STORAGE_KEY) as Lang | null;
  if (saved && saved in dicts) return saved;
  const nav = navigator.language.toLowerCase();
  if (nav.startsWith("ru")) return "ru";
  if (nav.startsWith("uk")) return "uk";
  if (nav.startsWith("de")) return "de";
  return "en";
}

export function saveLang(lang: Lang) {
  localStorage.setItem(STORAGE_KEY, lang);
}

export function makeT(lang: Lang) {
  return (key: string, params?: Record<string, string | number>): string => {
    let s = dicts[lang][key] ?? dicts.en[key] ?? key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        s = s.replaceAll(`{${k}}`, String(v));
      }
    }
    return s;
  };
}

/** Локаль для форматирования чисел. */
export function numberLocale(lang: Lang): string {
  return { ru: "ru-RU", en: "en-US", uk: "uk-UA", de: "de-DE" }[lang];
}
