//! Collection of tools for scanning Google Suite for secrets. Currently only supports Google Drive.
//!
//! `GoogleScanner` acts as a wrapper around a [`SecretScanner`] object to provide helper functions for
//! performing scanning against Google Drive files. Relies on the
//! [`google_drive3`] library which provides a wrapper around the Google Drive API.
//!
//! # Examples
//!
//! Basic usage requires you to create a [`GDriveScanner`] object:
//!
//! ```
//! use rusty_hogs::google_scanning::GDriveScanner;
//!
//! let gs = GDriveScanner::new();
//! ```
//!
//! Alternatively you can customize the way the secret scanning will work by building
//! a [`SecretScanner`] object and supplying it to the [`GDriveScanner`] constructor:
//!
//! ```
//! use rusty_hogs::SecretScannerBuilder;
//! use rusty_hogs::google_scanning::GDriveScanner;
//! let ss = SecretScannerBuilder::new().set_pretty_print(true).build();
//! let gs = GDriveScanner::new_from_scanner(ss);
//! ```
//!
//! The next step is to create an authenticated [`DriveHub`] object and use it to create a
//! [`GDriveFileInfo`] object.
//!
//! Lastly, pass all these objects to the [`perform_scan`] method of [`GDriveScanner`].
//!
//! ```no_run
//! use rusty_hogs::SecretScannerBuilder;
//! use rusty_hogs::google_scanning::{GDriveScanner, GDriveFileInfo};
//! # use yup_oauth2::{ApplicationSecret, DiskTokenStorage, Authenticator, DefaultAuthenticatorDelegate, FlowType};
//! # use std::path::Path;
//! use google_drive3::DriveHub;
//!
//! // Initialize some variables
//! # let oauthsecretfile = "clientsecret.json";
//! # let oauthtokenfile = "temp_token";
//! let gdrive_scanner = GDriveScanner::new();
//!
//! // Start with GDrive auth - based on example code from drive3 API and yup-oauth2
//! # let secret: ApplicationSecret =
//! #     yup_oauth2::read_application_secret(Path::new(oauthsecretfile)).expect(oauthsecretfile);
//! # let token_storage = DiskTokenStorage::new(&String::from(oauthtokenfile)).unwrap();
//! # let auth = Authenticator::new(
//! #     &secret,
//! #     DefaultAuthenticatorDelegate,
//! #     hyper::Client::with_connector(hyper::net::HttpsConnector::new(
//! #         hyper_rustls::TlsClient::new(),
//! #     )),
//! #     token_storage,
//! #     Some(FlowType::InstalledInteractive),
//! # );
//! let hub = DriveHub::new(
//!     hyper::Client::with_connector(hyper::net::HttpsConnector::new(
//!         hyper_rustls::TlsClient::new(),
//!     )),
//!     auth,
//! );
//!
//! // get some initial info about the file
//! let gdriveinfo = GDriveFileInfo::new("gdrive_file_id", &hub).unwrap();
//!
//! // Do the scan
//! let findings = gdrive_scanner.perform_scan(&gdriveinfo, &hub, false);
//! gdrive_scanner.secret_scanner.output_findings(&findings);
//! ```
//!
//! [`SecretScanner`]: ../struct.SecretScanner.html
//! [`google_drive3`]: https://docs.rs/google-drive3/1.0.12+20190620/google_drive3/
//! [`DriveHub`]: https://docs.rs/google-drive3/1.0.12+20190620/google_drive3/struct.DriveHub.html
//! [`GDriveScanner`]: struct.GDriveScanner.html
//! [`GDriveFileInfo`]: struct.GDriveFileInfo.html
//! [`perform_scan`]: struct.GDriveScanner.html#method.perform_scan

use crate::SecretScanner;
use encoding::all::ASCII;
use encoding::{DecoderTrap, Encoding};
use google_drive3::{DriveHub, Scope};
use hyper::Client;
use serde_derive::{Deserialize, Serialize};
use simple_error::SimpleError;
use std::collections::HashSet;
use std::io::Read;
use std::iter::FromIterator;
use yup_oauth2::{Authenticator, DefaultAuthenticatorDelegate, DiskTokenStorage};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, Default)]
/// `serde_json` object that represents a single found secret - finding
///
/// ```
/// # use rusty_hogs::google_scanning::GDriveFinding;
/// let gdf: GDriveFinding = GDriveFinding {
///    date: String::from("2019-12-21T16:32:31+00:00"),
///    diff: String::from("context around finding"),
///    path: String::from("GDrive folder path"),
///    strings_found: Vec::new(),
///    g_drive_id: String::from("GDrive file ID"),
///    reason: String::from("Regex description"),
///    web_link: String::from("http://drive.google.com/docs/gdriveid")
/// };
/// ```
pub struct GDriveFinding {
    pub date: String,
    pub diff: String,
    pub path: String,
    #[serde(rename = "stringsFound")]
    pub strings_found: Vec<String>,
    pub g_drive_id: String,
    pub reason: String,
    pub web_link: String,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
/// Contains helper functions for performing scans of Google Drive objects
///
/// ```
/// # use rusty_hogs::google_scanning::GDriveScanner;
/// let gds: GDriveScanner = GDriveScanner::new();
/// ```
pub struct GDriveScanner {
    pub secret_scanner: SecretScanner,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Default)]
/// A helper object containing a set of strings describing a Google Drive file.
///
/// ```
/// # use rusty_hogs::google_scanning::GDriveFileInfo;
/// let gdfi: GDriveFileInfo = GDriveFileInfo {
///   file_id: String::from("GDrive file ID"),
///    mime_type: String::from("MIME"),
///    modified_time: String::from("context around finding"),
///    web_link: String::from("context around finding"),
///    parents: Vec::new(),
///    name: String::from("context around finding"),
///    path: String::from("context around finding")
/// };
/// ```
pub struct GDriveFileInfo {
    pub file_id: String,
    pub mime_type: String,
    pub modified_time: String,
    pub web_link: String,
    pub parents: Vec<String>,
    pub name: String,
    pub path: String,
}

impl GDriveFileInfo {

    /// Construct a `GDriveFileInfo` object from a Google Drive File ID and an authorized `DriveHub` object
    pub fn new(
        file_id: &str,
        hub: &DriveHub<
            Client,
            Authenticator<DefaultAuthenticatorDelegate, DiskTokenStorage, Client>,
        >,
    ) -> Result<Self, SimpleError> {
        let fields = "kind, id, name, mimeType, webViewLink, modifiedTime, parents";
        let hub_result = hub
            .files()
            .get(file_id)
            .add_scope(Scope::Readonly)
            .param("fields", fields)
            .doit();
        let (_, file_object) = match hub_result {
            Ok(x) => x,
            Err(e) => {
                return Err(SimpleError::new(format!(
                    "failed accessing Google Metadata API {:?}",
                    e
                )))
            }
        };

        // initialize some variables from the response
        let modified_time = file_object.modified_time.unwrap();
        let web_link = file_object.web_view_link.unwrap();
        let parents = file_object.parents.unwrap_or_else(Vec::new); //TODO: add code to map from id -> name
        let name = file_object.name.unwrap();
        let path = format!("{}/{}", parents.join("/"), name);
        let mime_type = match file_object.mime_type.unwrap().as_ref() {
            "application/vnd.google-apps.spreadsheet" => "text/csv", //TODO: Support application/x-vnd.oasis.opendocument.spreadsheet https://github.com/tafia/calamine
            "application/vnd.google-apps.document" => "text/plain",
            u => return Err(SimpleError::new(format!("unknown doc type {}", u))),
        };
        Ok(Self {
            file_id: file_id.to_owned(),
            mime_type: mime_type.to_owned(),
            modified_time,
            web_link,
            parents,
            name,
            path,
        })
    }
}

/// Acts as a wrapper around a `SecretScanner` object to provide helper functions for performing
/// scanning against Google Drive files. Relies on the [`google_drive3`](https://docs.rs/google-drive3/1.0.10+20190620/google_drive3/)
/// library which provides a wrapper around the Google Drive v3 API.
impl GDriveScanner {
    /// Initialize the `SecretScanner` object first using the `SecretScannerBuilder`, then provide
    /// it to this constructor method.
    pub fn new_from_scanner(secret_scanner: SecretScanner) -> Self {
        Self { secret_scanner }
    }

    pub fn new() -> Self { Self { secret_scanner: SecretScanner::default() } }

    /// Takes information about the file, and the DriveHub object, and retrieves the content from
    /// Google Drive. Expect authorization issues here if you don't have access to the file.
    fn gdrive_file_contents(
        gdrivefile: &GDriveFileInfo,
        hub: &DriveHub<
            Client,
            Authenticator<DefaultAuthenticatorDelegate, DiskTokenStorage, Client>,
        >,
    ) -> Result<Vec<u8>, SimpleError> {
        let resp_obj = hub
            .files()
            .export(&gdrivefile.file_id, &gdrivefile.mime_type)
            .doit();
        let mut resp_obj = match resp_obj {
            Ok(r) => r,
            Err(e) => return Err(SimpleError::new(e.to_string())),
        };
        let mut buffer: Vec<u8> = Vec::new();
        match resp_obj.read_to_end(&mut buffer) {
            Err(e) => return Err(SimpleError::new(e.to_string())),
            Ok(s) => s,
        };
        Ok(buffer)
    }

    /// Takes information about the file, and the DriveHub object, and return a list of findings.
    /// This calls get_file_contents(), so expect an HTTPS call to GDrive.
    pub fn perform_scan(
        &self,
        gdrivefile: &GDriveFileInfo,
        hub: &DriveHub<
            Client,
            Authenticator<DefaultAuthenticatorDelegate, DiskTokenStorage, Client>,
        >,
        scan_entropy: bool,
    ) -> HashSet<GDriveFinding> {
        // download an export of the file, split on new lines, store in lines
        let buffer = Self::gdrive_file_contents(gdrivefile, hub).unwrap();
        let lines = buffer.split(|x| (*x as char) == '\n');

        // main loop - search each line for secrets, output a list of GDriveFinding objects
        let mut findings: HashSet<GDriveFinding> = HashSet::new();
        for new_line in lines {
            let matches_map = self.secret_scanner.matches(&new_line);
            for (reason, match_iterator) in matches_map {
                let mut secrets: Vec<String> = Vec::new();
                for matchobj in match_iterator {
                    secrets.push(
                        ASCII
                            .decode(
                                &new_line[matchobj.start()..matchobj.end()],
                                DecoderTrap::Ignore,
                            )
                            .unwrap_or_else(|_| "<STRING DECODE ERROR>".parse().unwrap()),
                    );
                }
                if !secrets.is_empty() {
                    findings.insert(GDriveFinding {
                        diff: ASCII
                            .decode(&new_line, DecoderTrap::Ignore)
                            .unwrap_or_else(|_| "<STRING DECODE ERROR>".parse().unwrap()),
                        date: gdrivefile.modified_time.clone(),
                        strings_found: secrets.clone(),
                        reason: reason.clone(),
                        g_drive_id: gdrivefile.file_id.to_string(),
                        path: gdrivefile.path.clone(),
                        web_link: gdrivefile.web_link.clone(),
                    });
                }
            }

            if scan_entropy {
                let ef = SecretScanner::entropy_findings(new_line);
                if !ef.is_empty() {
                    findings.insert(GDriveFinding {
                        diff: ASCII
                            .decode(&new_line, DecoderTrap::Ignore)
                            .unwrap_or_else(|_| "<STRING DECODE ERROR>".parse().unwrap()),
                        date: gdrivefile.modified_time.clone(),
                        strings_found: ef,
                        reason: "Entropy".parse().unwrap(),
                        g_drive_id: gdrivefile.file_id.to_string(),
                        path: gdrivefile.path.clone(),
                        web_link: gdrivefile.web_link.clone(),
                    });
                }
            }
        }

        HashSet::from_iter(findings.into_iter())
    }
}

impl Default for GDriveScanner {
    fn default() -> Self {
        Self::new()
    }
}
