Expedition retained dossiers
============================

Base Camp retains an accepted dossier through DASObjectStore's fixed service
boundary. An operator does not create a DAS user, password, PAM account,
static grant file, S3 key, or reusable token. Sign in to Base Camp with Pistis;
the live Prosopikon authority must project a principal-scoped DAS Operate or
Administer entitlement.

The service writes beneath the configured ``expedition/dossiers`` prefix. A
successful Base Camp record includes the DAS-constructed canonical evidence
reference and the independent read-back receipt. If authority expires or DAS
storage is unavailable, the dossier remains unaccepted and the UI reports the
failed retention step. An exact retry is safe; changed bytes require a new
immutable dossier identity.
