package se.digg.wallet.r2ps.domain.mapper.json;

import com.fasterxml.jackson.core.JsonParser;
import com.fasterxml.jackson.databind.DeserializationContext;
import com.fasterxml.jackson.databind.JsonDeserializer;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import se.digg.wallet.r2ps.domain.event.Event;
import se.digg.wallet.r2ps.domain.event.ServerWalletRegistered;

import java.io.IOException;

public class EventDeserializer extends JsonDeserializer<Event> {

  @Override
  public Event deserialize(JsonParser p, DeserializationContext ctxt) throws IOException {
    JsonNode node = p.getCodec().readTree(p);

    // Extract eventType from nested metadata
    JsonNode metadataNode = node.get("metadata");
    if (metadataNode == null || !metadataNode.has("eventType")) {
      throw new IOException("Missing metadata.eventType");
    }

    String eventType = metadataNode.get("eventType").asText();

    // Determine the concrete class based on eventType
    Class<? extends Event> eventClass = switch (eventType) {
      case "ServerWalletRegistered" -> ServerWalletRegistered.class;
      // Add other event types here
      default -> throw new IllegalArgumentException("Unknown event type: " + eventType);
    };

    // Deserialize to the concrete type
    ObjectMapper mapper = (ObjectMapper) p.getCodec();
    return mapper.treeToValue(node, eventClass);
  }
}
