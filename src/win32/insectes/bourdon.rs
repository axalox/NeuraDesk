//! **LE BOURDONNEMENT** de la mouche et du moustique en vol, synthétisé au
//! premier envol (aucun fichier son à livrer) et joué en boucle par `waveOut`.
//!
//! - **Un canal par insecte** : `PlaySound` ne joue qu'un son à la fois pour
//!   tout le processus, or la mouche et le moustique peuvent voler ensemble.
//!   Chaque `Bourdon` ouvre son propre `HWAVEOUT`, que Windows mélange.
//! - **Une boucle sans raccord** : la fin du tampon est fondue dans son début
//!   (`boucler`), donc la reprise ne claque pas.
//! - **Le volume est dans les échantillons** (`GAIN_*`) : `waveOutSetVolume`
//!   touche le volume de tout le processus sur les Windows récents.

use std::sync::OnceLock;

use windows::core::PSTR;
use windows::Win32::Media::Audio::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutUnprepareHeader, waveOutWrite, CALLBACK_NULL,
    HWAVEOUT, WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_BEGINLOOP, WHDR_ENDLOOP,
};

use super::Espece;

pub const FREQUENCE: u32 = 22_050;
/// La longueur de la boucle : assez longue pour que la variation du ton ne
/// s'entende pas revenir.
const DUREE_BOUCLE_S: f32 = 3.0;
/// Le fondu de raccord, en échantillons (~90 ms).
const FONDU: usize = 2_000;
/// L'attaque, en échantillons (~40 ms) : le son ne démarre pas d'un coup.
const ATTAQUE: usize = 900;
const GAIN_MOUCHE: f32 = 0.22;
const GAIN_MOUSTIQUE: f32 = 0.12;

/// Un générateur pseudo-aléatoire fixe : le même son à chaque lancement.
struct Graine(u64);

impl Graine {
    fn suivant(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
}

/// Un bruit lent et lisse (de -1 à 1) : le ton et le volume qui flottent.
fn derive(graine: &mut Graine, n: usize, lissage: f32) -> Vec<f32> {
    let mut v = Vec::with_capacity(n);
    let (mut x, mut vitesse) = (0.0f32, 0.0f32);
    for _ in 0..n {
        vitesse = vitesse * (1.0 - lissage) + graine.suivant() * lissage;
        x = (x + vitesse * lissage * 4.0).clamp(-1.0, 1.0) * 0.9995;
        v.push(x);
    }
    v
}

/// Le son brut, un peu plus long que la boucle (pour le fondu), de -1 à 1.
fn synthese(espece: Espece, n: usize) -> Vec<f32> {
    let mut graine = Graine(match espece {
        Espece::Mouche(_) => 0x5EED_F1E5,
        _ => 0x5EED_0A0A,
    });
    // (fréquence de base, écart du ton, harmoniques, pente des harmoniques, souffle)
    let (f0, ecart, harmoniques, pente, souffle) = match espece {
        Espece::Mouche(_) => (205.0f32, 22.0f32, 14, 0.85f32, 0.10f32),
        _ => (610.0, 45.0, 7, 1.35, 0.04),
    };
    let ton = derive(&mut graine, n, 0.0008);
    let volume = derive(&mut graine, n, 0.002);
    let mut phase = 0.0f32;
    let mut souffle_filtre = 0.0f32;
    let mut sortie = Vec::with_capacity(n);
    let norme: f32 = (1..=harmoniques).map(|k| 1.0 / (k as f32).powf(pente)).sum();
    for i in 0..n {
        let f = f0 + ecart * ton[i];
        phase = (phase + f / FREQUENCE as f32).fract();
        let mut s = 0.0;
        for k in 1..=harmoniques {
            s += (phase * k as f32 * std::f32::consts::TAU).sin() / (k as f32).powf(pente);
        }
        s /= norme;
        // Le souffle des ailes : un bruit grave, battu au rythme du ton.
        souffle_filtre += (graine.suivant() - souffle_filtre) * 0.08;
        s += souffle * souffle_filtre * (0.5 + 0.5 * (phase * std::f32::consts::TAU).sin());
        // Le volume qui respire : l'insecte vire, s'approche, s'éloigne.
        s *= 0.78 + 0.22 * volume[i];
        sortie.push(s.clamp(-1.0, 1.0));
    }
    sortie
}

/// Boucle `brut` (de longueur `n + fondu`) sur `n` échantillons : le début
/// reçoit la suite de la fin en fondu enchaîné, donc `sortie[n-1]` → `sortie[0]`
/// enchaîne sans saut.
pub fn boucler(brut: &[f32], fondu: usize) -> Vec<f32> {
    let n = brut.len() - fondu;
    let mut v = brut[..n].to_vec();
    for (i, x) in v.iter_mut().take(fondu).enumerate() {
        let t = i as f32 / fondu as f32;
        *x = brut[i] * t + brut[n + i] * (1.0 - t);
    }
    v
}

/// La boucle d'une espèce, en PCM 16 bits, calculée une fois.
pub fn boucle(espece: Espece) -> &'static [i16] {
    static MOUCHE: OnceLock<Vec<i16>> = OnceLock::new();
    static MOUSTIQUE: OnceLock<Vec<i16>> = OnceLock::new();
    let (cellule, gain) = match espece {
        Espece::Mouche(_) => (&MOUCHE, GAIN_MOUCHE),
        _ => (&MOUSTIQUE, GAIN_MOUSTIQUE),
    };
    cellule.get_or_init(|| {
        let n = (DUREE_BOUCLE_S * FREQUENCE as f32) as usize;
        let brut = synthese(espece, n + FONDU);
        boucler(&brut, FONDU).iter().map(|x| (x * gain * i16::MAX as f32) as i16).collect()
    })
}

/// L'attaque : les premiers échantillons de la boucle, montant de zéro. La
/// boucle reprend ensuite là où l'attaque s'arrête.
pub fn attaque(boucle: &[i16]) -> (Vec<i16>, Vec<i16>) {
    let n = ATTAQUE.min(boucle.len() / 2);
    let debut = boucle[..n].iter().enumerate().map(|(i, &x)| (x as f32 * i as f32 / n as f32) as i16).collect();
    // La boucle tournée : elle commence où l'attaque finit, et reste continue.
    let mut suite = boucle[n..].to_vec();
    suite.extend_from_slice(&boucle[..n]);
    (debut, suite)
}

/// Un bourdonnement qui joue tant qu'il existe.
pub struct Bourdon {
    sortie: HWAVEOUT,
    // Les tampons et leurs en-têtes restent EN PLACE (boîtes) jusqu'à la
    // fermeture : `waveOut` les lit pendant la lecture.
    _tampons: [Box<[i16]>; 2],
    entetes: [Box<WAVEHDR>; 2],
}

impl Bourdon {
    pub fn jouer(espece: Espece) -> Option<Bourdon> {
        let (debut, suite) = attaque(boucle(espece));
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: FREQUENCE,
            nAvgBytesPerSec: FREQUENCE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut sortie = HWAVEOUT(std::ptr::null_mut());
        let code = unsafe { waveOutOpen(Some(&mut sortie as *mut HWAVEOUT), WAVE_MAPPER, &format, None, None, CALLBACK_NULL) };
        if code != 0 {
            crate::diag!("[insectes] waveOutOpen a échoué ({code})");
            return None;
        }
        let mut tampons: [Box<[i16]>; 2] = [debut.into_boxed_slice(), suite.into_boxed_slice()];
        let entete = |t: &mut Box<[i16]>, drapeaux: u32, boucles: u32| {
            Box::new(WAVEHDR {
                lpData: PSTR(t.as_mut_ptr() as *mut u8),
                dwBufferLength: (t.len() * 2) as u32,
                dwFlags: drapeaux,
                dwLoops: boucles,
                ..Default::default()
            })
        };
        let [t0, t1] = &mut tampons;
        let entetes = [entete(t0, 0, 0), entete(t1, WHDR_BEGINLOOP | WHDR_ENDLOOP, u32::MAX)];
        let mut b = Bourdon { sortie, _tampons: tampons, entetes };
        let taille = std::mem::size_of::<WAVEHDR>() as u32;
        for e in &mut b.entetes {
            let p: *mut WAVEHDR = &mut **e;
            let code = unsafe { waveOutPrepareHeader(b.sortie, p, taille) };
            let code = if code == 0 { unsafe { waveOutWrite(b.sortie, p, taille) } } else { code };
            if code != 0 {
                crate::diag!("[insectes] waveOutWrite a échoué ({code})");
                return None; // `Drop` referme.
            }
        }
        Some(b)
    }
}

impl Drop for Bourdon {
    fn drop(&mut self) {
        let taille = std::mem::size_of::<WAVEHDR>() as u32;
        unsafe {
            let _ = waveOutReset(self.sortie);
            for e in &mut self.entetes {
                let _ = waveOutUnprepareHeader(self.sortie, &mut **e, taille);
            }
            let _ = waveOutClose(self.sortie);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_boucle_reprend_sans_saut() {
        for espece in [Espece::Mouche(0), Espece::Moustique(0)] {
            let b = boucle(espece);
            assert_eq!(b.len(), (DUREE_BOUCLE_S * FREQUENCE as f32) as usize);
            // Le plus grand pas d'un échantillon au suivant, dans la boucle...
            let pas_max = b.windows(2).map(|w| (w[1] as i32 - w[0] as i32).abs()).max().unwrap();
            // ... borne le pas du raccord, de la fin au début.
            let raccord = (b[0] as i32 - b[b.len() - 1] as i32).abs();
            assert!(raccord <= pas_max, "{espece:?} : raccord {raccord} > pas max {pas_max}");
            let crete = b.iter().map(|x| (*x as i32).abs()).max().unwrap();
            let gain = if espece.est_mouche() { GAIN_MOUCHE } else { GAIN_MOUSTIQUE };
            assert!(crete as f32 <= gain * i16::MAX as f32 + 1.0, "{espece:?} : crête {crete}");
            assert!(crete as f32 > gain * i16::MAX as f32 * 0.3, "{espece:?} : presque muet");
        }
    }

    #[test]
    fn boucler_fond_la_fin_dans_le_debut() {
        // Une rampe qui monte : la boucle doit reprendre là où la fin s'arrête.
        let brut: Vec<f32> = (0..14).map(|i| i as f32).collect();
        let v = boucler(&brut, 4);
        assert_eq!(v.len(), 10);
        assert_eq!(v[0], 10.0, "le début est la suite de la fin");
        assert_eq!(&v[4..], &brut[4..10]);
    }

    #[test]
    fn l_attaque_monte_de_zero_et_la_suite_reste_une_boucle() {
        let b: Vec<i16> = (0..4000).map(|i| ((i as f32 * 0.05).sin() * 8000.0) as i16).collect();
        let (debut, suite) = attaque(&b);
        assert_eq!(debut[0], 0);
        assert_eq!(debut.len() + suite.len(), b.len() + debut.len());
        // L'attaque finit juste avant le début de la suite.
        assert_eq!(suite[0], b[debut.len()]);
        assert_eq!(*suite.last().unwrap(), b[debut.len() - 1]);
    }

    #[test]
    fn la_mouche_est_plus_grave_que_le_moustique() {
        // Nombre de passages par zéro sur la boucle : ~2 par période du fondamental.
        let passages = |b: &[i16]| b.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        assert!(passages(boucle(Espece::Mouche(0))) * 2 < passages(boucle(Espece::Moustique(0))));
    }
}
