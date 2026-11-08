//! **OÙ PASSE LA MÉMOIRE DU PROCESSUS** — la sonde qui nomme les régions.
//!
//! Écrite le 2026-10-08 après une mesure qui a mis le doigt sur un chiffre sans
//! savoir quoi en faire : le processus PRINCIPAL de NeuraDesk tient **~2,2 Go**
//! de mémoire privée, **et ce chiffre ne bouge pas** qu'on ait 0, 4 ou 29
//! panneaux CEF chargés. Ce n'est donc ni Chromium, ni une fuite — c'est un
//! pied fixe, et il fallait le nommer.
//!
//! **POURQUOI UNE SONDE `cargo test` ET PAS UN OUTIL.** `Get-Process` ne rend
//! qu'un total ; `VMMap` n'est pas installé ; et `Add-Type` est bloqué par
//! l'antivirus sur cette machine (voir la fiche du dépôt). `VirtualQueryEx`, lui,
//! est un simple appel `kernel32` — et il est **en LECTURE SEULE** : la sonde
//! ouvre le processus avec `PROCESS_QUERY_INFORMATION | PROCESS_VM_READ`, jamais
//! en écriture, et ne touche à rien.
//!
//! **CE QU'ELLE RÉPOND** : la répartition des régions COMMIT / PRIVÉES par
//! taille, et les plus grosses nommées. Une texture plein écran 4K en BGRA fait
//! **33,2 Mo** : si l'on voit beaucoup de régions de cette taille, c'est du
//! rendu ; si l'on voit une seule région énorme, c'est un tampon unique.
//!
//! ⚠️ **LE PID VIENT DE `NEURADESK_PID`** : la sonde ne devine pas quel
//! processus viser. Elle le DIT et s'arrête si la variable manque, plutôt que de
//! tomber sur le processus courant — qui est celui du test, et ne prouverait
//! rien.

/// Le pid à examiner — **exigé**, voir la note de module.
const VARIABLE_PID: &str = "NEURADESK_PID";

/// Un mébioctet, pour l'affichage.
const MIO: f64 = 1024.0 * 1024.0;

/// Une région commit / privée, telle que `VirtualQueryEx` la rend.
struct Region {
    base: usize,
    /// **L'`AllocationBase` EST LE DISCRIMINANT** : plusieurs régions qui la
    /// partagent viennent d'UN SEUL `VirtualAlloc` — c'est une réserve unique
    /// découpée, pas des allocations indépendantes. Deux mêmes tailles avec deux
    /// bases différentes ne sont donc PAS le même objet.
    allocation: usize,
    taille: usize,
    protection: u32,
}

/// **LA SONDE — `cargo test --bins -- --ignored --nocapture ou_va_la_memoire`**.
///
/// ⚠️ `NEURADESK_PID=<pid>` est **exigé** : voir la doc de module.
#[test]
#[ignore]
fn ou_va_la_memoire() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{VirtualQueryEx, MEMORY_BASIC_INFORMATION};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };

    let Ok(brut) = std::env::var(VARIABLE_PID) else {
        panic!("{VARIABLE_PID} est exigé — ex. : NEURADESK_PID=3428 cargo test …");
    };
    let Ok(pid) = brut.trim().parse::<u32>() else {
        panic!("{VARIABLE_PID}={brut:?} n'est pas un pid");
    };

    let Ok(processus) =
        (unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) })
    else {
        panic!("OpenProcess({pid}) a échoué — le pid existe-t-il, et nous appartient-il ?");
    };

    // ⚠️ **L'ESPACE D'ADRESSAGE 64 BITS SE PARCOURT JUSQU'AU BOUT**, et sur un
    // processus qui réserve autant que Chromium, cela fait beaucoup d'appels.
    // On avance TOUJOURS de `RegionSize`, jamais d'un octet : une région
    // `MEM_FREE` de 100 Go se saute d'un coup.
    let mut regions: Vec<Region> = Vec::new();
    let mut adresse: usize = 0;
    let mut info = MEMORY_BASIC_INFORMATION::default();
    loop {
        let lus = unsafe {
            VirtualQueryEx(
                processus,
                Some(adresse as *const std::ffi::c_void),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if lus == 0 {
            break;
        }
        let taille = info.RegionSize;
        if taille == 0 {
            break;
        }
        // `State` et `Type` sont des newtypes : on lit leur valeur brute.
        // MEM_COMMIT = 0x1000, MEM_PRIVATE = 0x20000.
        if info.State.0 == 0x1000 && info.Type.0 == 0x20000 {
            regions.push(Region {
                base: adresse,
                allocation: info.AllocationBase as usize,
                taille,
                protection: info.Protect.0,
            });
        }
        match adresse.checked_add(taille) {
            Some(suivante) if suivante > adresse => adresse = suivante,
            _ => break,
        }
    }
    let _ = unsafe { CloseHandle(processus) };

    let total: usize = regions.iter().map(|r| r.taille).sum();
    println!("\n=== pid {pid} — regions COMMIT + PRIVEES ===");
    println!("  {} region(s), total {:.1} Mo", regions.len(), total as f64 / MIO);

    // **LA RÉPARTITION PAR TAILLE EST LE CŒUR DE LA SONDE** : c'est elle qui
    // distingue « beaucoup de textures » de « un seul gros tampon ».
    let mut histogramme: Vec<(usize, usize)> = Vec::new();
    for region in &regions {
        match histogramme.iter_mut().find(|(t, _)| *t == region.taille) {
            Some((_, n)) => *n += 1,
            None => histogramme.push((region.taille, 1)),
        }
    }
    histogramme.sort_by_key(|(t, n)| std::cmp::Reverse(*t * *n));
    println!("\n  --- par TAILLE (taille x nombre = poids) ---");
    for (taille, nombre) in histogramme.iter().take(15) {
        println!(
            "    {taille:>12} o  x{nombre:<5} = {:>8.1} Mo   ({:.1} Mo l'unite)",
            (*taille * *nombre) as f64 / MIO,
            *taille as f64 / MIO
        );
    }

    let mut grosses = regions.iter().collect::<Vec<_>>();
    grosses.sort_by_key(|r| std::cmp::Reverse(r.taille));
    println!("\n  --- les 15 plus GROSSES regions ---");
    for region in grosses.iter().take(15) {
        println!(
            "    base 0x{:012x}  allocation 0x{:012x}  {:>8.1} Mo  prot 0x{:02x}",
            region.base,
            region.allocation,
            region.taille as f64 / MIO,
            region.protection
        );
    }

    // ⭐ **COMBIEN D'ALLOCATIONS DISTINCTES DERRIÈRE LES RÉGIONS ?** C'est la
    // question qui distingue « un seul tampon découpé en tranches » de
    // « n allocations indépendantes » — et donc un réservoir d'un objet répété.
    let mut par_allocation: Vec<(usize, usize, usize)> = Vec::new();
    for region in &regions {
        match par_allocation.iter_mut().find(|(a, _, _)| *a == region.allocation) {
            Some((_, n, octets)) => {
                *n += 1;
                *octets += region.taille;
            }
            None => par_allocation.push((region.allocation, 1, region.taille)),
        }
    }
    par_allocation.sort_by_key(|(_, _, octets)| std::cmp::Reverse(*octets));
    println!(
        "\n  --- par ALLOCATION D'ORIGINE ({} distincte(s)) ---",
        par_allocation.len()
    );
    for (allocation, nombre, octets) in par_allocation.iter().take(10) {
        println!(
            "    0x{allocation:012x}  {nombre:>5} region(s)  {:>8.1} Mo",
            *octets as f64 / MIO
        );
    }
    // **LES FENÊTRES DU MÊME PROCESSUS** — parce que la question suivante est
    // « une surface par fenêtre ? ». Un dock qui crée une cible de rendu par
    // fenêtre pleine largeur multiplie vite les mébioctets, et le compte des
    // fenêtres est ce qui le dit.
    println!("\n=== fenetres de premier niveau du pid {pid} ===");
    PID_VISE.store(pid, std::sync::atomic::Ordering::Relaxed);
    let mut fenetres: Vec<(isize, String, i32, i32)> = Vec::new();
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::EnumWindows(
            Some(collecter_fenetre),
            windows::Win32::Foundation::LPARAM(&mut fenetres as *mut _ as isize),
        )
    };
    let mut total_fenetres = 0usize;
    let mut aire: i64 = 0;
    for (_, classe, largeur, hauteur) in &fenetres {
        if *largeur <= 0 || *hauteur <= 0 {
            continue;
        }
        total_fenetres += 1;
        aire += (*largeur as i64) * (*hauteur as i64);
        if *largeur as i64 * *hauteur as i64 > 1_000_000 {
            println!("    {largeur:>5} x {hauteur:<5}  {classe}");
        }
    }
    println!(
        "    {total_fenetres} fenetre(s) avec une aire non nulle — {:.1} Mo si chacune etait un tampon BGRA",
        aire as f64 * 4.0 / MIO
    );
    println!();
}

/// Le rappel d'`EnumWindows` — il ne retient que les fenêtres du pid visé.
///
/// ⚠️ **LE PID À VISER EST DANS UN STATIQUE, PAS DANS `lparam`** : `EnumWindows`
/// prend un `LPARAM` (`isize`), et y faire voyager à la fois le pid et le vecteur
/// demanderait de la transmutation pour rien. Un statique lu par le rappel est
/// plus court et suffisant — la sonde tourne sur un seul fil.
static PID_VISE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "system" fn collecter_fenetre(
    fenetre: windows::Win32::Foundation::HWND,
    donnee: windows::Win32::Foundation::LPARAM,
) -> windows::core::BOOL {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindowRect, GetWindowThreadProcessId,
    };

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(fenetre, Some(&mut pid)) };
    if pid != PID_VISE.load(std::sync::atomic::Ordering::Relaxed) {
        return windows::core::BOOL(1);
    }
    let mut rect = windows::Win32::Foundation::RECT::default();
    let _ = unsafe { GetWindowRect(fenetre, &mut rect) };
    let mut tampon = [0u16; 128];
    let n = unsafe { GetClassNameW(fenetre, &mut tampon) };
    let classe = String::from_utf16_lossy(&tampon[..n.max(0) as usize]);
    let vecteur = unsafe { &mut *(donnee.0 as *mut Vec<(isize, String, i32, i32)>) };
    vecteur.push((fenetre.0 as isize, classe, rect.right - rect.left, rect.bottom - rect.top));
    windows::core::BOOL(1)
}
