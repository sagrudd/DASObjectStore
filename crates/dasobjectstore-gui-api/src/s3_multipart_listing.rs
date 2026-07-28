use dasobjectstore_daemon::api::ProfileS3MultipartUploadView;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MultipartListingQuery {
    prefix: String,
    key_marker: Option<String>,
    upload_id_marker: Option<String>,
    max_uploads: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MultipartListingPage {
    query: MultipartListingQuery,
    uploads: Vec<ProfileS3MultipartUploadView>,
    is_truncated: bool,
    next_key_marker: Option<String>,
    next_upload_id_marker: Option<String>,
}

impl MultipartListingQuery {
    pub(super) fn parse(query: &[(String, String)]) -> Result<Self, String> {
        let prefix = query_value(query, "prefix").unwrap_or_default().to_string();
        let key_marker = nonempty_query_value(query, "key-marker");
        let upload_id_marker = nonempty_query_value(query, "upload-id-marker");
        if upload_id_marker.is_some() && key_marker.is_none() {
            return Err("upload-id-marker requires key-marker".to_string());
        }
        let max_uploads = match query_value(query, "max-uploads") {
            None | Some("") => 1_000,
            Some(value) => value
                .parse::<usize>()
                .ok()
                .filter(|value| (1..=1_000).contains(value))
                .ok_or_else(|| "max-uploads must be an integer from 1 to 1000".to_string())?,
        };
        Ok(Self {
            prefix,
            key_marker,
            upload_id_marker,
            max_uploads,
        })
    }

    pub(super) fn apply(
        self,
        mut uploads: Vec<ProfileS3MultipartUploadView>,
    ) -> MultipartListingPage {
        uploads.sort_by(|left, right| {
            left.key
                .object_id
                .cmp(&right.key.object_id)
                .then_with(|| {
                    left.initiated_at_unix_seconds
                        .cmp(&right.initiated_at_unix_seconds)
                })
                .then_with(|| left.reservation_id.cmp(&right.reservation_id))
        });
        uploads.retain(|upload| upload.key.object_id.starts_with(&self.prefix));
        self.apply_marker(&mut uploads);
        let is_truncated = uploads.len() > self.max_uploads;
        uploads.truncate(self.max_uploads);
        let (next_key_marker, next_upload_id_marker) = if is_truncated {
            uploads
                .last()
                .map(|upload| {
                    (
                        Some(upload.key.object_id.clone()),
                        Some(upload.reservation_id.clone()),
                    )
                })
                .unwrap_or_default()
        } else {
            (None, None)
        };
        MultipartListingPage {
            query: self,
            uploads,
            is_truncated,
            next_key_marker,
            next_upload_id_marker,
        }
    }

    fn apply_marker(&self, uploads: &mut Vec<ProfileS3MultipartUploadView>) {
        let Some(key_marker) = self.key_marker.as_deref() else {
            return;
        };
        if let Some(upload_id_marker) = self.upload_id_marker.as_deref() {
            if let Some(marker_index) = uploads.iter().position(|upload| {
                upload.key.object_id == key_marker
                    && upload.reservation_id.as_str() == upload_id_marker
            }) {
                uploads.drain(..=marker_index);
                return;
            }
        }
        uploads.retain(|upload| upload.key.object_id.as_str() > key_marker);
    }
}

pub(super) fn render_multipart_upload_listing(bucket: &str, page: MultipartListingPage) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListMultipartUploadsResult><Bucket>{}</Bucket><KeyMarker>{}</KeyMarker><UploadIdMarker>{}</UploadIdMarker><NextKeyMarker>{}</NextKeyMarker><NextUploadIdMarker>{}</NextUploadIdMarker><Prefix>{}</Prefix><MaxUploads>{}</MaxUploads><IsTruncated>{}</IsTruncated>",
        xml_escape(bucket),
        xml_escape(page.query.key_marker.as_deref().unwrap_or_default()),
        xml_escape(page.query.upload_id_marker.as_deref().unwrap_or_default()),
        xml_escape(page.next_key_marker.as_deref().unwrap_or_default()),
        xml_escape(page.next_upload_id_marker.as_deref().unwrap_or_default()),
        xml_escape(&page.query.prefix),
        page.query.max_uploads,
        page.is_truncated
    );
    for upload in page.uploads {
        let initiated = dasobjectstore_core::utc::format_utc_timestamp_seconds(
            upload.initiated_at_unix_seconds as i64,
        );
        xml.push_str(&format!(
            "<Upload><Key>{}</Key><UploadId>{}</UploadId><Initiated>{}</Initiated></Upload>",
            xml_escape(&upload.key.object_id),
            xml_escape(&upload.reservation_id),
            initiated
        ));
    }
    xml.push_str("</ListMultipartUploadsResult>");
    xml
}

fn query_value<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.as_str())
}

fn nonempty_query_value(query: &[(String, String)], name: &str) -> Option<String> {
    query_value(query, name)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::backend::BackendObjectKey;

    fn upload(key: &str, upload_id: &str) -> ProfileS3MultipartUploadView {
        upload_at(key, upload_id, 1_700_000_000)
    }

    fn upload_at(
        key: &str,
        upload_id: &str,
        initiated_at_unix_seconds: u64,
    ) -> ProfileS3MultipartUploadView {
        ProfileS3MultipartUploadView {
            reservation_id: upload_id.to_string(),
            key: BackendObjectKey {
                object_id: key.to_string(),
                version: 1,
            },
            initiated_at_unix_seconds,
            completion: None,
        }
    }

    #[test]
    fn parses_prefix_markers_and_bound() {
        let query = vec![
            ("prefix".to_string(), "EPICv1/".to_string()),
            ("key-marker".to_string(), "EPICv1/a".to_string()),
            ("upload-id-marker".to_string(), "u-1".to_string()),
            ("max-uploads".to_string(), "25".to_string()),
        ];
        assert_eq!(
            MultipartListingQuery::parse(&query).expect("valid listing"),
            MultipartListingQuery {
                prefix: "EPICv1/".to_string(),
                key_marker: Some("EPICv1/a".to_string()),
                upload_id_marker: Some("u-1".to_string()),
                max_uploads: 25,
            }
        );
        assert!(MultipartListingQuery::parse(&[(
            "upload-id-marker".to_string(),
            "u-1".to_string()
        )])
        .is_err());
        assert!(
            MultipartListingQuery::parse(&[("max-uploads".to_string(), "1001".to_string())])
                .is_err()
        );
    }

    #[test]
    fn filters_sorts_and_paginates_by_both_markers() {
        let uploads = vec![
            upload("other/object", "u-9"),
            upload("EPICv1/b", "u-1"),
            upload("EPICv1/a", "u-2"),
            upload("EPICv1/a", "u-1"),
            upload("EPICv1/c", "u-1"),
        ];
        let first = MultipartListingQuery {
            prefix: "EPICv1/".to_string(),
            key_marker: None,
            upload_id_marker: None,
            max_uploads: 2,
        }
        .apply(uploads.clone());
        assert_eq!(
            first
                .uploads
                .iter()
                .map(|upload| (
                    upload.key.object_id.as_str(),
                    upload.reservation_id.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![("EPICv1/a", "u-1"), ("EPICv1/a", "u-2")]
        );
        assert!(first.is_truncated);

        let second = MultipartListingQuery {
            prefix: "EPICv1/".to_string(),
            key_marker: first.next_key_marker,
            upload_id_marker: first.next_upload_id_marker,
            max_uploads: 2,
        }
        .apply(uploads);
        assert_eq!(
            second
                .uploads
                .iter()
                .map(|upload| upload.key.object_id.as_str())
                .collect::<Vec<_>>(),
            vec!["EPICv1/b", "EPICv1/c"]
        );
        assert!(!second.is_truncated);
    }

    #[test]
    fn orders_same_key_by_initiation_before_opaque_upload_id() {
        let page = MultipartListingQuery {
            prefix: "EPICv1/".to_string(),
            key_marker: None,
            upload_id_marker: None,
            max_uploads: 1,
        }
        .apply(vec![
            upload_at("EPICv1/a", "aaa", 200),
            upload_at("EPICv1/a", "zzz", 100),
        ]);
        assert_eq!(page.uploads[0].reservation_id, "zzz");
        assert_eq!(page.next_upload_id_marker.as_deref(), Some("zzz"));
    }

    #[test]
    fn xml_echoes_effective_prefix_and_pagination() {
        let page = MultipartListingQuery {
            prefix: "EPIC&v1/".to_string(),
            key_marker: Some("EPIC&v1/a".to_string()),
            upload_id_marker: Some("u<1".to_string()),
            max_uploads: 1,
        }
        .apply(vec![
            upload("EPIC&v1/a", "u<1"),
            upload("EPIC&v1/a", "u<2"),
            upload("EPIC&v1/b", "u-3"),
        ]);
        let xml = render_multipart_upload_listing("bucket", page);
        assert!(xml.contains("<Prefix>EPIC&amp;v1/</Prefix>"));
        assert!(xml.contains("<KeyMarker>EPIC&amp;v1/a</KeyMarker>"));
        assert!(xml.contains("<UploadIdMarker>u&lt;1</UploadIdMarker>"));
        assert!(xml.contains("<NextUploadIdMarker>u&lt;2</NextUploadIdMarker>"));
        assert!(xml.contains("<MaxUploads>1</MaxUploads><IsTruncated>true</IsTruncated>"));
    }
}
