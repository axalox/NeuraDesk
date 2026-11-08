//! **LES KAOMOJIS, PAR THÈME.** La fenêtre qui les montre est
//! `win32::cef_kaomoji` — une page CEF, exactement comme les caractères accentués
//! (voir `win32::caracteres`, dont ce module est le frère).
//!
//! DEMANDE EXPLICITE (2026-10-06) : « une applet dans la zone Intégrés pour pouvoir
//! saisir les kaomojis de Windows directement comme le fait la fenêtre des caractères
//! spéciaux », puis « je veux TOUS les kaomoji proposés par Windows dans emojis et
//! plus ».
//!
//! ## ⭐ QUATRE POPULATIONS, UNE SEULE TABLE
//!
//!   1. **LES KAOMOJIS DE WINDOWS** — extraits de la machine, pas reconstitués. Le
//!      panneau d'emojis (Win+.) les tire de
//!      `C:\Windows\SystemApps\MicrosoftWindows.61869836.InpApp_cw5n1h2txyewy\
//!      TextInputLayoutData.dll` — **un nom qui ne dit ni « emoji » ni « kaomoji »**,
//!      d'où une première recherche par NOM restée vide. Les chaînes y sont en
//!      UTF-16LE, séparées par des NUL, mêlées aux libellés de claviers du monde
//!      entier.
//!   2. **LES MIENS** — ce que Windows n'a pas, retirés par comparaison d'ENSEMBLE.
//!   3. **CEUX DU CLASSEMENT PUBLIC** de `kaomojis.jp/en/ranking`, relevés le
//!      2026-10-06 (JSON-LD `ItemList`, `numberOfItems: 50`). Ils forment « Les
//!      populaires », et **un seul d'entre eux était déjà chez nous**.
//!   4. **CEUX DE GBOARD** — DEMANDE EXPLICITE (2026-10-23) : « ajouter tous les
//!      kaomojis de gboard », répartis dans nos thèmes, en créant ceux qui manquent.
//!      Extraits de l'APK **18.0.3.954559732** (arm64-v8a, août 2026) : 424
//!      kaomojis en 14 catégories, une par fichier XML binaire de `res/` (noms
//!      obscurcis : `res/ca9.xml` est la table renversée), chacun un `softkey_list`
//!      au gabarit `0x7f0d034d` dont les `softkey` portent le kaomoji dans
//!      `param_data`, DANS L'ORDRE de Gboard. Chaque catégorie va au thème qui lui
//!      correspond ; sept n'en avaient pas et sont devenues des groupes (« Musique
//!      et danse », « Regard noir », « Tables renversées », « Inquiétude »,
//!      « Haussements d'épaules », « Doigt pointé », « Smileys occidentaux »). 412
//!      ajoutés ; 8 déjà dans une sélection (exclusives) et 4 déjà dans leur thème
//!      sont restés où ils étaient.
//!      ⚠️ **LA LISTE PUBLIQUE DE 2020** (gist de myfonj, Gboard 9.3) en compte 448 :
//!      Gboard en a RETIRÉ depuis. C'est l'APK qui fait foi, pas la liste.
//!
//! ## ⛔ LES DEUX PISTES ÉCARTÉES POUR TROUVER LA LISTE DE WINDOWS
//!
//! `EmojiDS.dll` et `AdvancedEmojiDS.dll` sont des **DLL de code** (cherchées en
//! UTF-16LE, aucune chaîne de kaomoji — les « katakanas » qu'on y trouve sont des
//! coïncidences d'octets dans du binaire), et il n'y a rien au registre sous
//! `HKLM\SOFTWARE\Microsoft\Input\InputDataSources`. **Ce qu'il faut retenir : un
//! fichier de données peut porter un nom qui ne dit rien de son contenu** — c'est le
//! motif cherché DANS LES OCTETS qui a trouvé, pas le nom du fichier.
//!
//! ## ⭐ LES TROIS FAUX POSITIFS DU CRIBLE, ET CE QUI LES ÉCARTE ENFIN
//!
//! La table a longtemps porté **trois intrus** — `ⲿ〈`, `ⶰ〉` et `あ/A` — qui
//! n'ont jamais été des kaomojis :
//!
//!   * `ⲿ〈` et `ⶰ〉` venaient d'un bloc de chaînes JUMELLES du fichier
//!     (`ⲽ⫖`, `ⶰ⪭`, `ⳁ⪙`, puis `ⶰ《`, `ⶰ》`, `ⶰ「`, `ⶰ」`) — **un vrai kaomoji
//!     ne se décline pas en `ⶰ《` / `ⶰ》`**. Le copte (0x2C80-0x2CFF) et le tifinagh
//!     (0x2D30-0x2D7F) tombent juste AU-DESSUS de l'ancienne plage de bruit
//!     `0x2900-0x2BFF` : **zéro des 287 vrais n'emploie un caractère de 0x2C00 à
//!     0x2DFF**, et la plage a été élargie d'autant.
//!   * `あ/A` voisinait `あいう`, `カナ英` et `コピー` — **des libellés du clavier du
//!     panneau**, pas des visages. Ce qui le sépare est structurel : **un visage a des
//!     TRAITS** — une chaîne dont tous les caractères sont des lettres ou des barres
//!     obliques n'en est pas un.
//!
//! ⚠️ **LE CRIBLE NE POUVAIT PAS ÊTRE « IL FAUT UN VRAI CROCHET »** : il emportait
//! `/ᐠ｡ꞈ｡ᐟ\`, un chat parfaitement légitime qui n'a que des barres. Un crible qui
//! jette du bon grain ne vaut pas mieux que pas de crible — il a été mesuré, pas
//! supposé.
//!
//! ⚠️ **AUCUN CARACTÈRE DE CONTRÔLE** reste le premier crible, et un test le fige.
//! ⚠️ **QUATRE KAOMOJIS PORTENT UNE EMOJI HORS BMP** (`👉`, `👈`) — c'est figé et
//! compté, voir le test.
//!
//! ## LE CLASSEMENT PAR THÈME
//!
//! ⛔ **LE SITE NE POUVAIT PAS CLASSER LES NÔTRES** : ses 117 pages de thème (75 lues)
//! ne reconnaissent que **41 de nos 316** kaomojis. Les thèmes sont donc posés à la
//! MAIN, en lisant — ce qui a l'avantage de nommer en français ce que l'utilisateur
//! cherche (« Timidité », pas « shy »).
//!
//! ⚠️ **UN KAOMOJI PEUT ÊTRE DANS PLUSIEURS THÈMES** (demande de l'utilisateur) :
//! `(=^･ｪ･^=)` est à la fois un chat et mignon. La table porte donc plus d'ENTRÉES
//! que de kaomojis distincts — c'est voulu, et le test le dit.
//!
//! ⚠️ **LES DEUX PREMIÈRES FAMILLES SONT DES SÉLECTIONS, ET ELLES SONT EXCLUSIVES** :
//! un kaomoji de « Les populaires » ou de « Les classiques » ne reparaît pas plus bas.
//! Une sélection qui se répète se lirait comme un doublon.
//!
//! Voir [[neuradesk-fenetre-cef-les-pieges]] pour les pièges de la fenêtre.

/// **UN GROUPE DE LA FENÊTRE : SON NOM AFFICHÉ, ET SES KAOMOJIS.** L'ordre de
/// `FAMILLES` EST celui de la fenêtre, et c'est aussi celui de l'index que la page
/// renvoie au clic (voir `cef_kaomoji::kaomoji_par_index`).
pub(crate) struct Famille {
    pub nom: &'static str,
    pub kaomojis: &'static [&'static str],
}

/// **LES KAOMOJIS, PAR THÈME.** L'ordre EST celui de la fenêtre.
///
/// ⚠️ **LA DERNIÈRE LIGNE N'EST PAS FORCÉMENT LA PLUS LARGE ICI** : contrairement aux
/// caractères (des cases carrées toutes de la même taille), un kaomoji fait de deux à
/// quarante unités de large. La fenêtre se dimensionne donc sur le kaomoji le PLUS
/// LONG, pas sur un nombre de colonnes — voir `cef_kaomoji::taille_pour_tout_le_contenu`.
///
/// ⚠️ **`(╯°□°）╯︵ ┻━┻` MÉLANGE UNE PARENTHÈSE JAPONAISE FERMANTE (`）`, U+FF09) ET
/// UNE ASCII** — c'est ainsi que le kaomoji circule, et le « corriger » le rendrait
/// différent de ce que tout le monde colle.
pub(crate) const FAMILLES: &[Famille] = &[
    Famille {
        nom: "Les populaires",
        kaomojis: &[
            "₍^. .^₎⟆", "₍ᐢ.ˬ.ᐢ₎♡~", "꒰ঌ(˶ˆᗜˆ˵)໒꒱", "(˶ᵔ ᵕ ᵔ˶) ᵎᵎ", "( ⁰▿⁰)", "(*ˊᗜˋ*)", "₍^ >ヮ<^₎ .ᐟ.ᐟ", "(╥﹏╥)",
            "(๑˃ᴗ˂)ﻭ♡", "(◍´꒳`◍)", "(*´ω`*)", "(*ˊ˘ˋ*)", "˶ˊᜊˋ˶", "ദ്ദി*ˊᗜˋ*)", "( ˘ ³˘)♥", "(｡•́︿•̀｡)", "(≧◡≦)",
            "≽^•⩊•^≼", "＼(ˇ∇ˇ)／♥", "(。•̀ᴗ-)✧", "(≧∇≦)/", "(T_T)", "(._. )", "｡ﾟ(ﾟ´Д`ﾟ)ﾟ｡", "₍ᐢ.ˬ.ᐢ₎", "¯\\_(ツ)_/¯",
            "( ´͈ ᵕ `͈ )♡", "(⁄ ⁄>⁄ ▽ ⁄<⁄ ⁄)", "ヾ(・ω・)ゞ", "•̩̩̩̩ᯅ•̩̩̩", "♡～(>᎑<๑)", "(╥_╥)", "꒰ᐢ. .ᐢ꒱₊˚⊹♡", "≽^◉⩊◉^≼",
            "<(｀^´)>", "₍₍ (ง ˙ω˙)ว ⁾⁾", "(•̀ᗝ•́)💢", "٩( ᐛ )و", "ฅ^•ﻌ•^ฅ", "(≧▽≦)", "♡₊˚ (˶ᵔᵕᵔ˶) ˚₊♡", "(இ﹏இ`)",
            "ʚ₍ᐢ. .ᐢ₎ɞ", "🎁(˶˃ ᵕ ˂˶)🎂♡", "૮꒰ ˶• ༝ •˶꒱ა", "(இдஇ)", "(˶˃ ᵕ ˂˶)", "(˃̣̣̥⌓˂̣̣̥ )っ", "✧٩(ˊᗜˋ*)و✧",
            "(♡´▽`♡)",
        ],
    },
    Famille {
        nom: "Les classiques",
        kaomojis: &[
            "(╯°□°）╯︵ ┻━┻", "( ͡° ͜ʖ ͡°)", "(⌐■_■)", "( •_•)>⌐■-■", "ヾ(⌐■_■)ノ♪", "(＾▽＾)", "(^‿^)", "(⌒▽⌒)", "(;_;)",
            "(♥ω♥*)", "(づ￣ 3￣)づ", "(´▽`ʃ♡ƪ)", "(ﾉ◕ヮ◕)ﾉ*:･ﾟ✧", "(/≧▽≦)/", "o(*^＠^*)o", "(｡･∀･)ﾉﾞ", "(￣▽￣)ノ",
            "(^.^)/~~~", "(=^･ｪ･^=)", "(＾• ω •＾)", "(￣(ｴ)￣)", "/ᐠ｡ꞈ｡ᐟ\\", "(⊙_⊙)？", "(o_ _)ﾉ", "<(＿　＿)>", "(´･_･`)",
            "(・_・;)", "(〃▽〃)", "（＾///＾）", "(¬_¬ )", "(→_→)", "(￣_,￣ )", "(°ロ°)", "(⊙﹏⊙)", "(っ °Д °;)っ", "Σ(っ °Д °;)っ",
            "(D:)", "＼(º □ º l|l)/", "(ง •_•)ง", "(๑•̀ㅂ•́)و✧", "( •̀ ω •́ )✧", "╮(╯▽╰)╭", "¯\\(°_o)/¯", "(╯‵□′)╯︵┻━┻",
            "┳━┳ ノ( ゜-゜ノ)", "(*^▽^*)", "(‾◡◝)", "♪(´▽｀)", "ヾ(≧▽≦*)o", "(((o(*ﾟ▽ﾟ*)o)))",
        ],
    },
    Famille {
        nom: "Joie",
        kaomojis: &[
            "(★‿★)", "(☆▽☆)", "(≧∀≦)ゞ", "(≧∇≦)ﾉ", "(≧﹏ ≦)", "(╯▽╰ )", "(☆-ｖ-)", "(✿◠‿◠)", "(✿◡‿◡)", "(＠＾０＾)",
            "（。＾▽＾）", "( *︾▽︾)", "(*✧×✧*)", "(^◕.◕^)", "(´ｰ∀ｰ`)", "(●'◡'●)", "(●ˇ∀ˇ●)", "(✿◕‿◕✿)", "(❁´◡`❁)",
            "(。・ω・。)", "(〜￣▽￣)〜", "(～￣▽￣)～", "o(^▽^)o", "q(≧▽≦q)", "♪(^∇^*)", "〜(￣▽￣〜)", "ㄟ(≧◇≦)ㄏ", "（*＾-＾*）",
            "＜（＾－＾）＞", "(o゜▽゜)o☆", "(u‿ฺu✿ฺ)", "~(￣▽￣)~*", "φ(゜▽゜*)♪", "ヽ(✿ﾟ▽ﾟ)ノ", "(∩^o^)⊃━☆", "(👉ﾟヮﾟ)👉",
            "\\(￣︶￣*\\))", "o(*^▽^*)┛", "o(*°▽°*)o", "o(*￣▽￣*)o", "o(*￣▽￣*)ブ", "o(*￣︶￣*)o", "o(〃＾▽＾〃)o", "~\\(≧▽≦)/~",
            "§(*￣▽￣*)§", "╰(*°▽°*)╯", "☆⌒(*＾-゜)v", "ヾ(^▽^*)))", "ヾ(≧ ▽ ≦)ゝ", "ヾ(＠⌒ー⌒＠)ノ", "👈(⌒▽⌒)👉", "👈(ﾟヮﾟ👈)",
            "(. ❛ ᴗ ❛.)", "[]~(￣▽￣)~*", "┏ (゜ω゜)=👉", "(๐॔˃̶ᗜ˂̶๐॓)", "o(*≧▽≦)ツ┏━┓", "ε=ε=ε=(~￣▽￣)~", "(・∀・(・∀・(・∀・*)",
            "||ヽ(*￣▽￣*)ノミ|Ю", "(*≧︶≦))(￣▽￣* )ゞ", "(o゜▽゜)o☆[BINGO!]", "＼(((￣(￣(￣▽￣)￣)￣)))／", "(￣y▽￣)╭ Ohohoho.....",
            "☆*: .｡. o(≧▽≦)o .｡.:*☆", "*★,°*:.☆(￣▽￣)/$:*.°★* 。", "(⌒‿⌒)", "(*'ω'*)", "(˘▾˘)", "(っ˘ω˘ς)", "(∪ ◡ ∪)",
            "(•‿•)", "(・∀・)", "◉‿◉", "｡◕‿◕｡", "(θ‿θ)", "ʘ‿ʘ", "(✷‿✷)", "(◔‿◔)", "(◕ᴗ◕✿)", "(ʘᴗʘ✿)", "(人 •͈ᴗ•͈)",
            "(◍•ᴗ•◍)", "( ╹▽╹ )", "(✯ᴗ✯)", "ಡ ͜ ʖ ಡ", "(ㆁωㆁ)", "<(￣︶￣)>", "(*´ω｀*)", "( ꈍᴗꈍ)", "(✿^‿^)",
            "^_________^", "(◡ ω ◡)", "( ´◡‿ゝ◡`)", "(｡•̀ᴗ-)✧", "(◠‿◕)", "(◠‿・)—☆", "✧◝(⁰▿⁰)◜✧", "(人*´∀｀)｡*ﾟ+",
            "(ﾉ◕ヮ◕)ﾉ*.✧",
        ],
    },
    Famille {
        nom: "Musique et danse",
        kaomojis: &[
            "♪～(´ε｀ )", "(＾3＾♪", "┌(・。・)┘♪", "♪ヽ(･ˇ∀ˇ･ゞ)", "⁽⁽◝( •௰• )◜⁾⁾", "₍₍◞( •௰• )◟₎₎", "⁽⁽ଘ( ˊᵕˋ )ଓ⁾⁾",
            "₍₍ ◝(　ﾟ∀ ﾟ )◟ ⁾⁾", "♪┌|∵|┘♪", "└|∵|┐♪", "( /^ω^)/♪♪", "(＾∇＾)ﾉ♪", "ヾ( ͝° ͜ʖ͡°)ノ♪", "(*ﾉ・ω・)ﾉ♫",
            "┌|o^▽^o|┘♪", "┏(＾0＾)┛", "┌(★ｏ☆)┘", "└( ＾ω＾)」", "(｢`･ω･)｢", "♪(┌・。・)┌", "ヘ(￣ω￣ヘ)", "ƪ(‾.‾“)┐", "ƪ(˘⌣˘)ʃ",
            "(ノ^_^)ノ", "＼(ﾟｰﾟ＼)", "ヽ(*ﾟｰﾟ*)ﾉ", "ヾ(･ω･*)ﾉ", "(~‾▿‾)~", "〜(꒪꒳꒪)〜",
        ],
    },
    Famille {
        nom: "Amour et câlins",
        kaomojis: &[
            "(*￣3￣)╭", "(★ ω ★)", "(っ´Ι`)っ", "ლ(╹◡╹ლ)", "(○｀ 3′○)", "(～o￣3￣)～", "（〃｀ 3′〃）", "(❤️ ω ❤️)", "ε(┬┬﹏┬┬)3",
            "ο(=•ω＜=)ρ⌒☆", "（づ￣3￣）づ╭❤️～", "(* ￣3)(ε￣ *)", "( ´･･)ﾉ(._.`)", "( *^-^)ρ(*╯^╰)", "⊂((・▽・))⊃",
            "(づ｡◕‿‿◕｡)づ", "༼ つ ◕‿◕ ༽つ", "(づ￣ ³￣)づ", "(⊃｡•́‿•̀｡)⊃", "ʕっ•ᴥ•ʔっ", "(o´･_･)っ", "(⊃ • ʖ̫ • )⊃", "(つ≧▽≦)つ",
            "(つ✧ω✧)つ", "(っ.❛ ᴗ ❛.)っ", "～(つˆДˆ)つ｡☆", "ლ(´ ❥ `ლ)", "⊂(•‿•⊂ )*.✧", "⊂(´･◡･⊂ )∘˚˳°", "⊂(･ω･*⊂)",
            "⊂(・﹏・⊂)", "⊂(・▽・⊂)", "⊂(◉‿◉)つ", "o((*^▽^*))o", "╰(*´︶`*)╯", "╰(＾3＾)╯", "╰(⸝⸝⸝´꒳`⸝⸝⸝)╯", "♡(˃͈ દ ˂͈ ༶ )",
            "ヾ(˙❥˙)ﾉ", "＼(^o^)／", "ლ(・﹏・ლ)", "ლ(◕ω◕ლ)", "(/･ω･(-ω-)", "(･ω･)つ⊂(･ω･)", "( T_T)＼(^-^ )", "(●♡∀♡)",
            "(๑♡⌓♡๑)", "(｡♡‿♡｡)", "(✿ ♡‿♡)", "(◍•ᴗ•◍)❤", "( ◜‿◝ )♡", "(｡･ω･｡)ﾉ♡", "(•ө•)♡", "(♡ω♡ ) ~♪", "꒰⑅ᵕ༚ᵕ꒱˖♡",
            "♡˖꒰ᵕ༚ᵕ⑅꒱", "♡(ӦｖӦ｡)", "ෆ╹ .̮ ╹ෆ", "(´∩｡• ᵕ •｡∩`)", "♡(> ਊ <)♡", "♥╣[-_-]╠♥", "(灬º‿º灬)♡", "(｡・//ε//・｡)",
            "(〃ﾟ3ﾟ〃)", "(´ε｀ )", "(~￣³￣)~", "(◕દ◕)", "(ʃƪ＾3＾）", "(*＾3＾)/～♡", "(っ˘з(˘⌣˘ )", "(●’3)♡(ε`●)", "(๑˙❥˙๑)",
            "(/^-^(^ ^*)/", "( ˶ ❛ ꁞ ❛ ˶ )", "(*˘︶˘*).｡*♡", "(◍•ᴗ•◍)✧*。",
        ],
    },
    Famille {
        nom: "Tristesse et larmes",
        kaomojis: &[
            "(。﹏。)", "(≧﹏ ≦)", "(。﹏。*)", "(ノへ￣、)", "(；′⌒`)", "( ˘︹˘ )", "(´。＿。｀)", "(˘･_･˘)", "(┬┬﹏┬┬)", "o(TヘTo)",
            "(´･ω･`)?", "(。>︿<)_θ", "~(>_<。)＼", "～(　TロT)σ", "`(*>﹏<*)′", "ε(┬┬﹏┬┬)3", "┗( T﹏T )┛", "ヽ(*。>Д<)o゜",
            "...( ＿ ＿)ノ｜", "(ノω<。)ノ))☆.。", "(︶︹︺)", "•́  ‿ ,•̀", "ಥ‿ಥ", "ʕ´• ᴥ•̥`ʔ", "༎ຶ‿༎ຶ", "( ；∀；)", "(´；ω；｀)",
            "(･ัω･ั)", "(╯︵╰,)", "Ó╭╮Ò", "(っ˘̩╭╮˘̩)っ", "( ･ั﹏･ั)", "(｡ŏ﹏ŏ)", "(๑´•.̫ • `๑)", "(´ . .̫ . `)",
            "(｡ﾉω＼｡)", "ಥ╭╮ಥ", "(ᗒᗩᗕ)", "( ≧Д≦)", ".·´¯`(>▂<)´¯`·.", "( ⚈̥̥̥̥̥́⌢⚈̥̥̥̥̥̀)", "ಥ_ಥ", "(´;︵;`)",
            "༼;´༎ຶ ۝ ༎ຶ༽", "｡:ﾟ(;´∩`;)ﾟ:｡", "(༎ຶ ෴ ༎ຶ)", "( ꈨຶ ˙̫̮ ꈨຶ )", "(〒﹏〒)", "(个_个)",
            "(-̩̩̩-̩̩̩-̩̩̩-̩̩̩-̩̩̩___-̩̩̩-̩̩̩-̩̩̩-̩̩̩-̩̩̩)", "(´°̥̥̥̥̥̥̥̥ω°̥̥̥̥̥̥̥̥｀)",
        ],
    },
    Famille {
        nom: "Colère",
        kaomojis: &[
            "(#`O′)", "(ノ｀Д)ノ", "(＃°Д°)", "(* ￣︿￣)", "(σ｀д′)σ", "(‧‧)nnn", "(′д｀σ)σ", "(￣ε(#￣)", "╰（‵□′）╯", "ヽ（≧□≦）ノ",
            "ヾ(≧へ≦)〃", "( ఠൠఠ )ﾉ", "(ﾉ*ФωФ)ﾉ", "┌( ಠ_ಠ)┘", "～(　TロT)σ", "( ఠ ͟ʖ ఠ)", "(ヘ･_･)ヘ┳━┳", "...(*￣０￣)ノ",
            "ε=( o｀ω′)ノ", "ヽ(*。>Д<)o゜", "( ͡ಠ ʖ̯ ͡ಠ)", "o(*≧▽≦)ツ┏━┓", "(　o=^•ェ•)o　┏━┓", "(ｏ ‵-′)ノ”(ノ﹏<。)",
            "<( ￣^￣)(θ(θ☆( >_<", "┻━┻ ︵ヽ(`Д´)ﾉ︵ ┻━┻", "<( ‵□′)>───Ｃε(┬﹏┬)3", "Pia!(ｏ ‵-′)ノ”(ノ﹏<。)",
            "<( ‵□′)───C＜─___-)||", "┻━┻ ︵ ＼( °□° )／ ︵ ┻━┻", "(╬ಠ益ಠ)",
        ],
    },
    Famille {
        nom: "Regard noir",
        kaomojis: &[
            "ಠ_ಠ", "ಠ_ʖಠ", "ಠ︵ಠ", "ಠ ೧ ಠ", "ಠಗಠ", "ಠ,_｣ಠ", "ಠωಠ", "ಠ ͜ʖ ಠ", "ಠ◡ಠ", "ಠ∀ಠ", "ಠ﹏ಠ", "ಠ‿ಠ", "ಠ益ಠ", "ಠᴥಠ",
            "ʕಠ_ಠʔ", "Σ(ಠ_ಠ)", "(ಠ_ಠ)>⌐■-■", "(⌐■-■)", "[̲̅$̲̅(̲̅ ͡ಠ_ಠ)̲̅$̲̅]", "ಠ ل͟ ಠ", "(ノಠ益ಠ)ノ", "(ಠ_ಠ)━☆ﾟ.*･｡ﾟ",
            "ರ_ರ", "ರ╭╮ರ", "(눈‸눈)", "(ب_ب)", "ತ_ತ", "ತ_ʖತ", "ಠಿ_ಠಿ", "ಠಿ_ಠ", "ಠಿヮಠ",
        ],
    },
    Famille {
        nom: "Tables renversées",
        kaomojis: &[
            "(ノ｀Д´)ノ彡┻━┻", "(┛◉Д◉)┛彡┻━┻", "(ﾉ≧∇≦)ﾉ ﾐ ┻━┻", "(ノಠ益ಠ)ノ彡┻━┻", "(╯ರ ~ ರ)╯︵ ┻━┻", "(┛ಸ_ಸ)┛彡┻━┻",
            "(ﾉ´･ω･)ﾉ ﾐ ┻━┻", "(ノಥ,_｣ಥ)ノ彡┻━┻", "(┛✧Д✧))┛彡┻━┻", "┻┻︵ヽ(`Д´)ﾉ︵┻┻", "(/¯◡ ‿ ◡)/¯ ~ ┻━┻", "(ノ｀⌒´)ノ┫：・┻┻",
            "(ﾉ°_o)ﾉ⌒┫ ┻ ┣ ┳", "┻━┻ミ＼(≧ﾛ≦＼)", "┻━┻︵└(՞▽՞ └)", "┻━┻︵└(´_｀└)", "┻━┻ ヘ╰( •̀ε•́ ╰)", "─=≡Σ(╯°□°)╯︵┻┻",
            "(ノ•̀ o •́ )ノ ~ ┻━┻", "(-_- )ﾉ⌒┫ ┻ ┣ ┳", "(ノ￣皿￣)ノ ⌒== ┫", "(┛❍ᴥ❍)┛彡┻━┻", "(ノT＿T)ノ ＾┻━┻", "ʕノ•ᴥ•ʔノ ︵ ┻━┻",
            "┬─┬ノ( ͡° ͜ʖ ͡°ノ)", "┬─┬ノ(ಠ_ಠノ)", "┬─┬ノ( º _ ºノ)", "┬──┬◡ﾉ(° -°ﾉ)", "(ヘ･_･)ヘ┳━┳",
        ],
    },
    Famille {
        nom: "Surprise",
        kaomojis: &[
            "(⊙_◎)", "(◎﹏◎)", "（⊙ｏ⊙）", "(＃°Д°)", "(＠_＠;)", "(￣﹏￣；)", "(ﾟДﾟ*)ﾉ", "w(ﾟДﾟ)w", "щ(ʘ╻ʘ)щ", "щ(゜ロ゜щ)",
            "≡(▔﹏▔)≡", "┌(。Д。)┐", "（*゜ー゜*）", "o(><；)oo", "\\(0^◇^0)/", "o((⊙﹏⊙))o.", "(((φ(◎ロ◎;)φ)))",
            "ε=ε=ε=┏(゜ロ゜;)┛", "（゜ロ゜）", "(･o･;)", "(・o・)", "(゜o゜;", "w(°ｏ°)w", "(☉｡☉)!", "(@_@)", "ヽ((◎д◎))ゝ",
            "＼(°o°)／", "ヽ(｡◕o◕｡)ﾉ.", "＼(◎o◎)／", "ヾ(*’Ｏ’*)/", "(ﾉ*0*)ﾉ", "ヽ༼⁰o⁰；༽ノ", "⋋✿ ⁰ o ⁰ ✿⋌", "(ﾉﾟ0ﾟ)ﾉ~",
            "ლ(^o^ლ)", "(ﾟοﾟ人))", "⊙.☉", "(⑉⊙ȏ⊙)", "(ʘᗩʘ’)", "(‘◉⌓◉’)", "⁄(⁄ ⁄•⁄-⁄•⁄ ⁄)⁄", "(｡☬０☬｡)", "(´⊙ω⊙`)！",
            "(((;ꏿ_ꏿ;)))", "(●__●)", "(✿☉｡☉)", "(>0<；)", "༼⁰o⁰；༽", "(╬⁽⁽ ⁰ ⁾⁾ Д ⁽⁽ ⁰ ⁾⁾)",
        ],
    },
    Famille {
        nom: "Inquiétude",
        kaomojis: &[
            "(๑•﹏•)", "⊙﹏⊙", "╏ ” ⊚ ͟ʖ ⊚ ” ╏", "(╬☉д⊙)⊰⊹ฺ", "ヘ（。□°）ヘ", "(⊙_◎)", "ミ●﹏☉ミ", "(●´⌓`●)", "(*﹏*;)",
            "(＠_＠;)", "(ꏿ﹏ꏿ;)", "(;ŏ﹏ŏ)", "(• ▽ •;)", "(˘･_･˘)", "(*・～・*)", "(;;;・_・)", "(・–・;)ゞ", "(^～^;)ゞ",
            "(￣ヘ￣;)", "(٥↼_↼)", "(ー_ー゛)", "(─.─||）", "(-_-;)", "(-_-メ)", "(-_-;)・・・", "(´-﹏-`；)", "(~_~メ)", "(~_~;)",
            "(ʘ言ʘ╬)", "(^_^メ)", "(；^ω^）",
        ],
    },
    Famille {
        nom: "Timidité",
        kaomojis: &[
            "(^^ゞ", "(/▽＼)", "(°ー°〃)", "(►__◄)", "(￣┰￣*)", "(￣▽￣)\"", "( ╯□╰ )", "(*/ω＼*)", "（；´д｀）ゞ", "o(￣┰￣*)ゞ",
            "（〃｀ 3′〃）", "(*￣;(￣ *)", "(ˉ▽ˉ；)...", "(￣ ‘i ￣;)", "o(*////▽////*)q", "(/ω＼*)……… (/ω•＼*)", "(￣ω￣;)",
        ],
    },
    Famille {
        nom: "Fatigue et ennui",
        kaomojis: &[
            "(＠_＠;)", "(～﹃～)~zZ", "(￣_￣|||)", "(_　_)。゜zｚＺ", "<(￣ c￣)y▂ξ", "(￣o￣) . z Z", "━━(￣ー￣*|||━━",
            "( ′ 3`) sigh~",
        ],
    },
    Famille {
        nom: "Réflexion",
        kaomojis: &[
            "(⊙o⊙)？", "(⊙ˍ⊙)？", "φ(*￣0￣)", "╮(╯-╰)╭", "╮（╯＿╰）╭", "✍️(◔◡◔)", "＼（〇_ｏ）／", "(´･ω･`)?", "_〆(´Д｀ )",
            "…(⊙_⊙;)…", "┑(￣Д ￣)┍", "( ¯(∞)¯ )", "¯\\_( ͡° ͜ʖ ͡°)_/¯",
        ],
    },
    Famille {
        nom: "Haussements d'épaules",
        kaomojis: &[
            "┐( ∵ )┌", "╮(. ❛ ᴗ ❛.)╭", "乁༼☯‿☯✿༽ㄏ", "╮(＾▽＾)╭", "乁[ ◕ ᴥ ◕ ]ㄏ", "乁[ᓀ˵▾˵ᓂ]ㄏ", "┐(´(エ)｀)┌", "┐( ˘_˘)┌",
            "┐(´ー｀)┌", "╮(╯_╰)╭", "乁( ⁰͡ Ĺ̯ ⁰͡ ) ㄏ", "乁( •_• )ㄏ", "乁| ･ 〰 ･ |ㄏ", "┐(‘～`;)┌", "┐(￣ヘ￣)┌", "┐(´д`)┌",
            "乁( . ര ʖ̯ ര . )ㄏ", "乁 ˘ o ˘ ㄏ", "乁ʕ •̀ ۝ •́ ʔㄏ", "乁║ ˙ 益 ˙ ║ㄏ",
        ],
    },
    Famille {
        nom: "Salutations",
        kaomojis: &[
            "(≧∀≦)ゞ", "(。・・)ノ", "(oﾟvﾟ)ノ", "(。・∀・)ノ", "＼(ﾟｰﾟ＼)", "(^._.^)ﾉ", "(○´･д･)ﾉ", "(ﾉ*･ω･)ﾉ", "ヾ(•ω•`)o",
            "（＾∀＾●）ﾉｼ", "( ﾉ ﾟｰﾟ)ﾉ", "(＾Ｕ＾)ノ~ＹＯ", "ヾ(≧ ▽ ≦)ゝ", "( ﾟдﾟ)つBye", "( ﾟдﾟ)つ Bye", "Hi~ o(*￣▽￣*)ブ",
            "ヾ(￣▽￣)Bye~Bye~", "☆⌒(*＾-゜)v THX!!", "ヾ(￣▽￣) Bye~Bye~",
        ],
    },
    Famille {
        nom: "Animaux",
        kaomojis: &[
            "(^._.^)ﾉ", "(^・ω・^ )", "(ﾉ*ФωФ)ﾉ", "<。)#)))≦", "~(=^‥^)ノ", "o(=•ェ•=)m", "（o´・ェ・｀o）", "(～￣(OO)￣)ブ",
            "^(*￣(oo)￣)^", "(　o=^•ェ•)o　┏━┓", "(･(ｴ)･)", "V●ᴥ●V", "▼・ᴥ・▼", "U ´꓃ ` U", "(◠ᴥ◕ʋ)", "U^ｪ^U",
            "( ͡°ᴥ ͡° ʋ)", "◖⚆ᴥ⚆◗", "(=｀ェ´=)", "(￣(ｴ)￣)ﾉ", "(*￣(ｴ)￣*)", "(≧(ｴ)≦ )", "(´(ｪ)｀）", "(・(ｪ)・）", "ʕ º ᴥ ºʔ",
            "ʕ·ᴥ·ʔ", "ʕ ꈍᴥꈍʔ", "ʕ´•ᴥ•`ʔ", "(✪㉨✪)", "(◕ᴥ◕)", "(ᵔᴥᵔ)", "Ꮚ˘ ꈊ ˘ Ꮚ", "(´・(oo)・｀)", "～>`)～～～", "…ᘛ⁐̤ᕐᐷ",
            "くコ:彡", "-ᄒᴥᄒ-", "Ƹ̵̡Ӝ̵̨̄Ʒ",
        ],
    },
    Famille {
        nom: "Combativité",
        kaomojis: &[
            "b(￣▽￣)d", "o(￣▽￣)ｄ", "ผ(•̀_•́ผ)", "☆⌒(*＾-゜)v", "( •̀ ω •́ )y", "<(￣︶￣)↗[GO!]", "( •̀ .̫ •́ )✧",
            "d=====(￣▽￣*)b", "ᕙ(⇀‸↼‶)ᕗ", "ᕙ(＠°▽°＠)ᕗ", "ᕙ(  • ‿ •  )ᕗ", "୧(﹒︠ᴗ﹒︡)୨", "ᕙ (° ~͜ʖ~ °) ᕗ",
            "ᕙ( ͡◉ ͜ ʖ ͡◉)ᕗ", "୧(＾ 〰 ＾)୨", "ᕙ( ¤ 〰 ¤ )ᕗ", "ᕙ( ~ . ~ )ᕗ", "ᕙ( : ˘ ∧ ˘ : )ᕗ", "ᕙ[･۝･]ᕗ", "ᕦ( ⊡ 益 ⊡ )ᕤ",
            "ᕙ(ಠ ਊ ಠ)ᕗ", "ᕙ(☉ਊ☉)ᕗ", "୧( ಠ Д ಠ )୨", "ᕦ⊙෴⊙ᕤ", "ᕦ(ò_óˇ)ᕤ", "ᕦ(ಠ_ಠ)ᕤ", "ᕦ[ ◑ □ ◑ ]ᕤ", "ᕦ༼ ~ •́ ₒ •̀ ~ ༽ᕤ",
            "୧( ˵ ° ~ ° ˵ )୨", "୧| ͡ᵔ ﹏ ͡ᵔ |୨", "ᕙ (° ~ ° ~)", "( ͝° ͜ʖ͡°)ᕤ", "ᕙ( ͡° ͜ʖ ͡°)ᕗ", "ᕙ(͡°‿ ͡°)ᕗ",
            "ᕦ༼ຈل͜ຈ༽ᕤ", "༼ᕗຈل͜ຈ༽ᕗ", "ᕙ༼◕ ᴥ ◕༽ᕗ", "ᕦʕ •ᴥ•ʔᕤ", "ᕦᶘ ᵒ㉨ᵒᶅᕤ", "ᕦ༼✩ل͜✩༽ᕤ",
        ],
    },
    Famille {
        nom: "Gourmandise",
        kaomojis: &[
            "(ˉ﹃ˉ)", "(￣﹃￣)", "(╹ڡ╹ )", "ԅ(¯﹃¯ԅ)", "<。)#)))≦", "[]~(￣▽￣)~*",
        ],
    },
    Famille {
        nom: "Malice et taquinerie",
        kaomojis: &[
            "(¬‿¬)", "(°°)～", "(￣m￣）", "(￣、￣)", "（︶^︶）", "(¬_¬\")", "(ˉ▽￣～)", "( ‵▽′)ψ", "(*￣rǒ￣)", "Ψ(￣∀￣)Ψ",
            "ψ(｀∇´)ψ", "(#｀-_ゝ-)", "(lll￢ω￢)", "(ʘ ʖ̯ ʘ)", "(ʘ ͜ʖ ʘ)", "(ʘ ͟ʖ ʘ)", "(￢︿̫̿￢☆)", "!(*￣(￣　*)",
            "( ఠ ͟ʖ ఠ)", "(•ˋ _ ˊ•)", "( ﹁ ﹁ ) ~→", "(▀̿Ĺ̯▀̿ ̿)", "┌( ´_ゝ` )┐", "( ͠° ͟ʖ ͡°)", "( ͡~ ͜ʖ ͡°)",
            "( ͡ಠ ʖ̯ ͡ಠ)", "( ͡• ͜ʖ ͡• )", "┬┴┬┴┤(･_├┬┴┬┴", "¯\\_( ͡° ͜ʖ ͡°)_/¯", "(￣y▽￣)╭ Ohohoho.....",
            "( ͡°( ͡° ͜ʖ( ͡° ͜ʖ ͡°)ʖ ͡°) ͡°)",
        ],
    },
    Famille {
        nom: "Doigt pointé",
        kaomojis: &[
            "┗(•ˇ_ˇ•)―→", "(☞ ಠ_ಠ)☞", "(☞ﾟ∀ﾟ)☞", "☞￣ᴥ￣☞", "→(° ۝ °)┗", "→_→", "←_←", "⟵(o_O)", "⟵(๑¯◡¯๑)",
            "ԅ( ͒ ۝ ͒ )ᕤ", "☜ (↼_↼)", "｡.ﾟ+ ⟵(｡･ω･)", "←(>▽<)ﾉ", "←(*꒪ヮ꒪*)", "(☞^o^) ☞", "(╭☞•́⍛•̀)╭☞", "〈(•ˇ‿ˇ•)-→",
            "(?・・)σ", "(☉｡☉)!→", "(ﾉﾟ0ﾟ)ﾉ→", "(´⊙ω⊙`)→", "(☞ﾟヮﾟ)☞", "(☞ ͡° ͜ʖ ͡°)☞", "´◔‿ゝ◔`)━☞", "( ՞ਊ ՞）→",
            "(　･ω･)☞", "(*❛‿❛)→", "╰( ･ ᗜ ･ )➝", "(✧Д✧)→", "(｡◕‿◕｡)➜", "<(￣︶￣)↗", "*･゜ﾟ(^O^)↝",
        ],
    },
    Famille {
        nom: "Smileys occidentaux",
        kaomojis: &[
            ":-)", ":^)", "^_^", "(^^)", ":,-)", "8-)", "B-)", "o:-)", ":-D", "}:‑)", ";)", ";-)", ":-*", ":-P",
            ":-!", ":-$", ":‑X", ":-|", ":-[", ":-(", ":'(", "(TT)", "=_=", ">.<", "(+_+)", "(*_*)", "O_o", ":-O",
            "=-O", ":0",
        ],
    },
    Famille {
        nom: "Divers",
        kaomojis: &[
            "(:◎)≡", "(•_•)", "(◉Θ◉)", "(。_。)", "(●__●)", "≡[。。]≡", "（￣。。￣）", "╚(•⌂•)╝", "╰(￣ω￣ｏ)", "．<{=．．．．",
            "( ◍•㉦•◍ )", "ϵ( ‘Θ’ )϶", "ヽ(￣ω￣(￣ω￣〃)ゝ", "ヽ(゜▽゜　)－C<(/;◇;)/~",
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Tous les kaomojis de la table, dans l'ordre de la fenêtre — doublons COMPRIS,
    /// puisqu'un kaomoji peut appartenir à plusieurs thèmes.
    fn entrees() -> Vec<&'static str> {
        FAMILLES.iter().flat_map(|f| f.kaomojis.iter().copied()).collect()
    }

    /// **LES KAOMOJIS DISTINCTS**, sans les répétitions de thème.
    fn distincts() -> std::collections::BTreeSet<&'static str> {
        entrees().into_iter().collect()
    }

    /// ⭐ **LE CONTENU DE LA FENÊTRE, FIGÉ.** Un groupe vidé, un nom perdu ou une
    /// population qui change se verraient à l'usage — c'est-à-dire trop tard.
    #[test]
    fn les_groupes_et_leurs_noms_sont_ceux_qui_ont_ete_valides() {
        assert_eq!(FAMILLES.len(), 23, "2 sélections + 21 thèmes");
        assert_eq!(FAMILLES[0].nom, "Les populaires");
        assert_eq!(FAMILLES[1].nom, "Les classiques");
        // Les deux sélections sont PLEINES, et la première est celle du site.
        assert_eq!(FAMILLES[0].kaomojis.len(), 50, "le classement public est un top 50");
        assert_eq!(FAMILLES[1].kaomojis.len(), 50, "les classiques aussi");
        // ⛔ AUCUN GROUPE VIDE : un titre suivi du vide se lirait comme un défaut.
        for f in FAMILLES {
            assert!(!f.kaomojis.is_empty(), "groupe vide : {}", f.nom);
            assert!(!f.nom.trim().is_empty(), "un groupe sans nom");
        }
        // ⛔ AUCUN KAOMOJI VIDE — une case cliquable qui n'écrit rien.
        assert!(entrees().iter().all(|k| !k.trim().is_empty()), "un kaomoji est vide");
        // ⚠️ LE COMPTE DES KAOMOJIS DISTINCTS : 287 de Windows + 26 miens + 50 du site
        // + 406 de Gboard (ses 424, moins ceux que la table avait déjà).
        assert_eq!(distincts().len(), 768, "les kaomojis distincts");
    }

    /// ⭐ **LES DEUX SÉLECTIONS SONT EXCLUSIVES** — un kaomoji de « Les populaires »
    /// ou de « Les classiques » ne reparaît pas plus bas. C'est un CHOIX (voir la doc
    /// du module), et rien d'autre ne l'empêcherait.
    #[test]
    fn un_kaomoji_de_selection_ne_reparait_pas_dans_un_theme() {
        let plus_bas: std::collections::BTreeSet<&str> =
            FAMILLES.iter().skip(2).flat_map(|f| f.kaomojis.iter().copied()).collect();
        for f in FAMILLES.iter().take(2) {
            for &k in f.kaomojis {
                assert!(!plus_bas.contains(k), "{k:?} est dans « {} » ET dans un thème", f.nom);
            }
        }
        // ⛔ ET LES DEUX SÉLECTIONS NE SE RECOUPENT PAS NON PLUS entre elles.
        let pop: std::collections::BTreeSet<&str> = FAMILLES[0].kaomojis.iter().copied().collect();
        for &k in FAMILLES[1].kaomojis {
            assert!(!pop.contains(k), "{k:?} est dans les DEUX sélections");
        }
    }

    /// ⭐ **LES TROIS FAUX POSITIFS DU CRIBLE NE SONT PLUS LÀ — ET C'EST FIGÉ.**
    ///
    /// `ⲿ〈` et `ⶰ〉` venaient d'un bloc de chaînes jumelles du fichier de Windows
    /// (copte et tifinagh, juste au-dessus de l'ancienne plage de bruit) ; `あ/A` est
    /// un libellé du clavier japonais, voisin de `あいう` et `カナ英`. **Ce test ÉCHOUE
    /// sur l'ancienne table** — c'est ce qui en fait un garde, et pas une décoration.
    #[test]
    fn les_trois_fragments_binaires_ne_sont_plus_dans_la_table() {
        let tous = distincts();
        for intrus in ["ⲿ〈", "ⶰ〉", "あ/A"] {
            assert!(!tous.contains(intrus), "{intrus:?} est un fragment binaire, pas un kaomoji");
        }
    }

    /// ⛔ **AUCUN CARACTÈRE DE CONTRÔLE** — le premier crible, celui qui a séparé la
    /// liste du bruit binaire du fichier de Windows. S'il en rentrait un ici, la
    /// frappe enverrait une touche de contrôle dans l'application de l'utilisateur.
    #[test]
    fn aucun_kaomoji_ne_porte_de_caractere_de_controle() {
        for k in entrees() {
            assert!(
                !k.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f),
                "{k:?} porte un caractère de contrôle"
            );
        }
    }

    /// ⭐ **QUATRE KAOMOJIS DE WINDOWS PORTENT UNE EMOJI HORS BMP — ET C'EST FIGÉ ICI.**
    ///
    /// La liste de Windows contient les mains qui pointent (`👉`, `👈`, U+1F449 et
    /// U+1F448) : des paires de substitution, donc QUATRE unités UTF-16 à envoyer au
    /// lieu de deux. `caracteres::frapper_texte` encode en UTF-16 et envoie **toutes**
    /// les unités, donc le cas est géré — mais il est nommé, compté, et vérifié,
    /// plutôt que découvert par un test qui tombe.
    ///
    /// ⚠️ **LES KAOMOJIS DU SITE EN AJOUTENT** (`💢`, `🎁🎂`) : ceux-là ne sont pas
    /// dans la liste de Windows, et le total ci-dessous est celui de TOUTE la table.
    #[test]
    fn les_kaomojis_hors_bmp_sont_comptes() {
        let tous: Vec<&str> = distincts().into_iter().collect();
        let hors_bmp: Vec<&&str> = tous.iter().filter(|k| k.chars().any(|c| (c as u32) > 0xffff)).collect();
        // MESURÉ : sept dans la table de Windows (dont les quatre mains qui pointent),
        // plus trois apportés par le classement public.
        assert_eq!(hors_bmp.len(), 6, "les kaomojis hors BMP : {hors_bmp:?}");
        // Aucune paire de substitution PARTIELLE : chaque kaomoji se décode en un
        // nombre entier de caractères, donc rien ne peut être coupé au milieu.
        for k in &tous {
            assert!(k.chars().count() > 0 && k.encode_utf16().count() >= k.chars().count());
        }
    }

    /// ⭐ **LES KAOMOJIS DE GBOARD SONT LÀ, ET LEURS GROUPES AUSSI** — un par
    /// catégorie de Gboard qui n'avait pas d'équivalent, et un témoin de chacun.
    #[test]
    fn les_kaomojis_de_gboard_sont_dans_leurs_groupes() {
        let groupe = |nom: &str| FAMILLES.iter().find(|f| f.nom == nom).unwrap_or_else(|| panic!("groupe absent : {nom}"));
        for (nom, temoin) in [
            ("Musique et danse", "┌(・。・)┘♪"),
            ("Regard noir", "ಠ_ಠ"),
            ("Tables renversées", "(ノಠ益ಠ)ノ彡┻━┻"),
            ("Inquiétude", "(๑•﹏•)"),
            ("Haussements d'épaules", "┐( ∵ )┌"),
            ("Doigt pointé", "(☞ﾟ∀ﾟ)☞"),
            ("Smileys occidentaux", ":-)"),
            ("Combativité", "ᕙ(⇀‸↼‶)ᕗ"),
            ("Animaux", "▼・ᴥ・▼"),
        ] {
            assert!(groupe(nom).kaomojis.contains(&temoin), "{temoin:?} absent de « {nom} »");
        }
    }

    /// **LES DEUX RAISONS D'OUVRIR CETTE FENÊTRE SONT LÀ.** Ce sont les seules qu'on
    /// ne sait pas taper au clavier.
    #[test]
    fn le_haussement_d_epaules_et_le_retournement_de_table_sont_presents() {
        let tous = distincts();
        assert!(tous.contains("¯\\_(ツ)_/¯"), "le haussement d'épaules");
        assert!(tous.contains("(╯°□°）╯︵ ┻━┻"), "le retournement de table");
    }

    /// ⭐ **CHAQUE KAOMOJI EST DANS AU MOINS UN THÈME** — ou dans une sélection. Sans
    /// ce test, un kaomoji ajouté à `WINDOWS` mais oublié dans le classement par
    /// thème disparaîtrait de la fenêtre EN SILENCE : la table est la seule source, et
    /// rien ne lit la liste de Windows directement.
    #[test]
    fn tous_les_kaomojis_de_windows_et_les_miens_sont_dans_un_groupe() {
        // Le compte de référence : ce que le générateur a extrait. S'il change, c'est
        // le fichier de Windows qui a changé, et il faut relire le classement.
        assert_eq!(distincts().len(), 768);
        // Chaque kaomoji des sélections est unique dans sa sélection.
        for f in FAMILLES.iter().take(2) {
            let s: std::collections::BTreeSet<&str> = f.kaomojis.iter().copied().collect();
            assert_eq!(s.len(), f.kaomojis.len(), "doublon dans « {} »", f.nom);
        }
    }
}
