// LE NUANCIER PARTAGÉ — celui de l'éditeur des lueurs (`lueurs.html`), et depuis
// le 2026-10-15 celui des terminaux (`terminal_theme.js`) : DEMANDE EXPLICITE,
// « mets la même palette de couleurs que pour les lueurs ».
//
// DEMANDE EXPLICITE (2026-10-14) : « beaucoup plus de couleurs disponibles ainsi
// que de variantes de noir ». 16 teintes (0, 15, 30, 45, 60, 80, 110, 140, 165,
// 185, 200, 215, 235, 260, 285, 320°) × 7 clartés, une rangée de gris du blanc au
// noir, et deux rangées de NOIRS TEINTÉS : une lueur noire ASSOMBRIT, ce qui
// donne des ombres colorées.
//
// ⚠️ Des CODES ÉCRITS EN CLAIR, et en minuscules comme ceux que Rust renvoie
// (`cef_lueurs::hex`) : c'est par égalité de texte que la pastille choisie
// s'allume, et `cef_terminal::tests` les relit pour vérifier leur lisibilité.
window.NUANCIER = [
  ["#ffcccc", "#ffd9cc", "#ffe6cc", "#fff2cc", "#ffffcc", "#eeffcc", "#d5ffcc", "#ccffdd", "#ccfff2", "#ccfbff", "#cceeff", "#cce1ff", "#ccd0ff", "#ddccff", "#f2ccff", "#ffccee"],
  ["#ff8f8f", "#ffab8f", "#ffc78f", "#ffe38f", "#ffff8f", "#daff8f", "#a2ff8f", "#8fffb4", "#8fffe3", "#8ff6ff", "#8fdaff", "#8fbeff", "#8f98ff", "#b48fff", "#e38fff", "#ff8fda"],
  ["#fb5151", "#fb7b51", "#fba651", "#fbd051", "#fbfb51", "#c2fb51", "#6dfb51", "#51fb89", "#51fbd0", "#51ecfb", "#51c2fb", "#5198fb", "#515ffb", "#8951fb", "#d051fb", "#fb51c2"],
  ["#ed1d1d", "#ed511d", "#ed851d", "#edb91d", "#eded1d", "#a7ed1d", "#3fed1d", "#1ded62", "#1dedb9", "#1ddbed", "#1da7ed", "#1d73ed", "#1d2eed", "#621ded", "#b91ded", "#ed1da7"],
  ["#b31919", "#b34019", "#b36619", "#b38c19", "#b3b319", "#80b319", "#33b319", "#19b34d", "#19b38c", "#19a6b3", "#197fb3", "#1959b3", "#1926b3", "#4c19b3", "#8c19b3", "#b31980"],
  ["#791515", "#792e15", "#794715", "#796015", "#797915", "#587915", "#267915", "#157937", "#157960", "#157179", "#155879", "#153f79", "#151e79", "#371579", "#601579", "#791558"],
  ["#4c1010", "#4c1f10", "#4c2e10", "#4c3d10", "#4c4c10", "#384c10", "#1a4c10", "#104c24", "#104c3d", "#10474c", "#10384c", "#10294c", "#10154c", "#24104c", "#3d104c", "#4c1038"],
  // Les gris, du blanc au noir.
  ["#ffffff", "#eeeeee", "#dddddd", "#cccccc", "#bbbbbb", "#aaaaaa", "#999999", "#888888", "#777777", "#666666", "#555555", "#444444", "#333333", "#222222", "#111111", "#000000"],
  // Les NOIRS TEINTÉS : à peine colorés, puis presque purs.
  ["#290a0a", "#29120a", "#291a0a", "#29210a", "#29290a", "#1f290a", "#0f290a", "#0a2914", "#0a2921", "#0a2629", "#0a1f29", "#0a1729", "#0a0d29", "#140a29", "#210a29", "#290a1f"],
  ["#120707", "#120a07", "#120d07", "#121007", "#121207", "#0f1207", "#091207", "#07120b", "#071210", "#071212", "#070f12", "#070c12", "#070812", "#0b0712", "#100712", "#12070f"],
];
